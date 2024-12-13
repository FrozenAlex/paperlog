use std::{
    cell::OnceCell,
    collections::HashMap,
    fs::{self, File, OpenOptions},
    io::{BufWriter, Write},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, RwLock,
    },
    thread,
    time::{Duration, Instant},
};

use crate::{log_level::LogLevel, semaphore_lite::SemaphoreLite, Result};
use color_eyre::{
    eyre::{bail, eyre, Context},
    owo_colors::colors::css::Gold,
};
use itertools::Itertools;

#[cfg(all(target_os = "android", feature = "logcat"))]
pub mod logcat_logger;

#[cfg(feature = "stdout")]
pub mod file_logger;

#[cfg(feature = "stdout")]
pub mod stdout_logger;

#[cfg(feature = "sinks")]
pub mod sink_logger;

mod log_data;
pub use log_data::LogData;

pub trait LogCallback: Fn(&LogData) -> Result<()> + Send + Sync {}

pub type ThreadSafeLoggerThread = Arc<RwLock<LoggerThread>>;

#[repr(C)]
#[derive(Debug, Clone)]
pub struct LoggerConfig {
    pub max_string_len: usize,
    pub log_max_buffer_count: usize,
    pub line_end: char,

    #[cfg(feature = "file")]
    pub context_log_path: PathBuf,
}

pub struct LoggerThread {
    pub config: LoggerConfig,

    log_queue: Arc<(SemaphoreLite, Mutex<Vec<LogData>>)>,
    flush_semaphore: Arc<SemaphoreLite>,

    inited: AtomicBool,

    #[cfg(feature = "file")]
    global_file: BufWriter<File>,
    context_map: HashMap<String, BufWriter<File>>,

    sinks: Vec<Box<dyn LogCallback>>,
}

impl LoggerThread {
    pub fn new(config: LoggerConfig, log_path: PathBuf) -> Result<Self> {
        let log_queue = Arc::new((SemaphoreLite::new(), Mutex::new(Vec::new())));
        let flush_semaphore = Arc::new(SemaphoreLite::new());

        #[cfg(feature = "file")]
        let global_file = {
            fs::create_dir_all(&config.context_log_path).with_context(|| {
                format!(
                    "Unable to make logging directory for contexts {}",
                    config.context_log_path.display()
                )
            })?;

            if let Some(parent) = log_path.parent() {
                fs::create_dir_all(parent).with_context(|| {
                    format!(
                        "Unable to make logging directory for global file {}",
                        config.context_log_path.display()
                    )
                })?;
            }

            let inner = File::create(&log_path).map_err(|e| {
                eyre!(
                    "Unable to create global file at {}: {}",
                    log_path.display(),
                    e.to_string()
                )
            })?;
            BufWriter::new(inner)
        };

        Ok(LoggerThread {
            config,
            log_queue,
            flush_semaphore,
            inited: AtomicBool::new(false),

            #[cfg(feature = "file")]
            global_file,

            #[cfg(feature = "file")]
            context_map: HashMap::new(),

            sinks: Vec::new(),
        })
    }

    pub fn init(self) -> Result<ThreadSafeLoggerThread> {
        if self.inited.load(Ordering::SeqCst) {
            bail!("LoggerThread already initialized");
        }

        self.inited.store(true, Ordering::SeqCst);

        let log_queue_clone = Arc::clone(&self.log_queue);
        let flush_semaphore_clone = Arc::clone(&self.flush_semaphore);
        let thread_safe_self: Arc<RwLock<LoggerThread>> = Arc::new(self.into());
        let thread_safe_self_clone = Arc::clone(&thread_safe_self);

        thread::spawn(move || {
            Self::log_thread(
                log_queue_clone,
                flush_semaphore_clone,
                thread_safe_self_clone,
            )
        });

        Ok(thread_safe_self)
    }

    pub fn is_inited(&self) -> &AtomicBool {
        &self.inited
    }

    pub fn get_queue(&self) -> &Mutex<Vec<LogData>> {
        &self.log_queue.1
    }

    pub fn get_sinks(&self) -> &Vec<Box<dyn LogCallback>> {
        &self.sinks
    }

    pub fn queue_log(
        &self,
        level: LogLevel,
        tag: Option<String>,
        message: String,
        file: String,
        line: u32,
    ) {
        let log_data = LogData {
            level,
            tag,
            message: message.to_string(),
            timestamp: Instant::now(),
            file: file.into(),
            line,
        };

        let (sempahore, queue) = self.log_queue.as_ref();

        queue.lock().unwrap().push(log_data);
        sempahore.signal();
    }

    #[cfg(feature = "backtrace")]
    #[inline(always)]
    pub fn backtrace(&self) -> Result<()> {
        use std::backtrace::Backtrace;

        let backtrace = Backtrace::capture();
        let backtrace_str = format!("{:?}", backtrace);

        self.queue_log(
            LogLevel::ERROR,
            None,
            backtrace_str,
            file!().into(),
            line!(),
        );

        Ok(())
    }

    pub fn add_context(&mut self, tag: &str) -> Result<()> {
        #[cfg(feature = "file")]
        {
            let log_path = self.config.context_log_path.join(tag).with_extension("log");
            let file = BufWriter::new(
                File::create(&log_path)
                    .map_err(|e| eyre!("Unable to create context file at {}", e.to_string()))?,
            );

            self.context_map.insert(tag.to_string(), file);
        }

        Ok(())
    }

    pub fn add_sink<F>(&mut self, sink: F)
    where
        F: LogCallback + 'static,
    {
        self.sinks.push(Box::new(sink));
    }

    fn log_thread(
        log_queue: Arc<(SemaphoreLite, Mutex<Vec<LogData>>)>,
        flush_semaphore: Arc<SemaphoreLite>,
        logger_thread: Arc<RwLock<LoggerThread>>,
    ) -> Result<()> {
        let mut logs_since_last_flush: usize = 0;
        let mut last_log_time = Instant::now();

        let log_mutex = &log_queue.1;
        let log_semaphore_lite = &log_queue.0;

        loop {
            let max_str_len = logger_thread.read().unwrap().config.max_string_len;

            let mut queue_locked = log_mutex.lock().unwrap();

            // move items from queue to local variable
            let queue = Vec::from_iter(queue_locked.drain(..));
            drop(queue_locked);

            if !queue.is_empty() {
                let len = queue.len();
                let split_logs = split_str_into_chunks(queue, max_str_len);

                for log in split_logs {
                    do_log(log, logger_thread.clone())?;
                }
                logs_since_last_flush += len;
            }

            let elapsed_time = last_log_time.elapsed() > Duration::from_secs(1);
            let exceeded_log_buffer = logs_since_last_flush > 50;

            if exceeded_log_buffer || elapsed_time {
                logs_since_last_flush = 0;
                last_log_time = Instant::now();
            }

            // wait for further logs if nothing left
            if log_mutex.lock().unwrap().is_empty() {
                flush_semaphore.signal();
                log_semaphore_lite.wait();
            }
        }
    }

    ///
    /// Waits indefinitely until the next queue is flushed
    /// May block until a log is called forth
    pub(crate) fn wait_for_flush(&self) {
        self.log_queue.0.wait();
    }
    pub(crate) fn wait_for_flush_timeout(&self, duration: Duration) {
        self.log_queue.0.wait_timeout(duration);
    }
}

/// Split log message by line endings and then split each line into chunks
fn split_str_into_chunks(queue: Vec<LogData>, max_str_len: usize) -> impl Iterator<Item = LogData> {
    queue.into_iter().flat_map(move |log| {
        // split log message by line endings
        log.message
            .split("\n")
            .flat_map(|s| {
                // split string into chunks
                s.chars()
                    .chunks(max_str_len)
                    .into_iter()
                    .map(|chunk| {
                        let chunk = chunk.collect::<String>();
                        LogData {
                            message: chunk,
                            ..log.clone()
                        }
                    })
                    .collect_vec()
            })
            .collect_vec()
    })
}

fn do_log(log: LogData, logger_thread: Arc<RwLock<LoggerThread>>) -> Result<()> {
    #[cfg(feature = "file")]
    file_logger::do_log(&log, logger_thread.clone())?;

    #[cfg(feature = "stdout")]
    stdout_logger::do_log(&log);

    #[cfg(all(target_os = "android", feature = "logcat"))]
    logcat_logger::do_log(&log);

    #[cfg(feature = "sinks")]
    sink_logger::do_log(&log, logger_thread)?;

    Ok(())
}
