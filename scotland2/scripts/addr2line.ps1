# Windows version
param($p1)

if (Test-Path "./ndkpath.txt")
{
    $NDKPath = Get-Content ./ndkpath.txt
} else {
    $NDKPath = $ENV:ANDROID_NDK_HOME
}

if ($p1)
{
    & $NDKPath\toolchains\llvm\prebuilt\linux-x86_64\bin\llvm-addr2line -e ./build/debug/libpaper2_scotland2.so $p1
}
else
{
	echo give at least 1 argument
}
