fn main() {
    println!("cargo::rustc-link-arg=-Wl,--build-id,-soname=libpaper2_scotland2.so");

    cdylib_link_lines::metabuild();
}