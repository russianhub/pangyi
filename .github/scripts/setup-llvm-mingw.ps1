# 下载固定版本的 llvm-mingw（msvcrt，x86_64 宿主），解到 tools/llvm-mingw。
# 该目录已在 .gitignore 里。缓存命中时跳过下载，仍会重写链接用的兼容库。
#
# i686-pc-windows-gnu 的 rustc 会传 -lgcc / -lgcc_eh / -lgcc_s，llvm-mingw 不提供这些文件名。
# libgcc.a 只放一个占位符号（真正的编译器运行时由 clang 自己加 compiler-rt）。
# libgcc_eh.a 与 libgcc_s.a 是静态 libunwind.a 的副本，避免装进 DLL 对 libunwind.dll 的依赖。
# __register_frame_info 不在 libunwind 里，由 ime/build/x86-stub.o 提供。
$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true

$Repo = if ($env:GITHUB_WORKSPACE) { $env:GITHUB_WORKSPACE } else { (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path }
Set-Location $Repo

$Url = "https://github.com/mstorsjo/llvm-mingw/releases/download/20261020/llvm-mingw-20261020-msvcrt-x86_64.zip"
$Expected = "52a3e3f584a74c3cde8ceb0c6960549683178e03b36818cb02b7ed109a68ff67"
$Dest = Join-Path $Repo "tools\llvm-mingw"
$Gcc = Join-Path $Dest "bin\i686-w64-mingw32-gcc.exe"

if (-not (Test-Path $Gcc)) {
    $zip = Join-Path $env:RUNNER_TEMP "llvm-mingw-20261020-msvcrt-x86_64.zip"
    $extract = Join-Path $env:RUNNER_TEMP "llvm-mingw-extract"
    Write-Host "downloading $Url"
    & curl.exe -fL --retry 5 --retry-delay 2 -o $zip $Url
    $got = (Get-FileHash -Algorithm SHA256 $zip).Hash.ToLower()
    if ($got -ne $Expected) {
        throw "llvm-mingw SHA256 mismatch: $got"
    }
    if (Test-Path $extract) { Remove-Item -Recurse -Force $extract }
    New-Item -ItemType Directory -Force -Path $extract | Out-Null
    & tar.exe -xf $zip -C $extract
    $inner = @(Get-ChildItem $extract -Directory)
    if ($inner.Count -ne 1) { throw "expected one top-level directory in the llvm-mingw zip" }
    if (Test-Path $Dest) { Remove-Item -Recurse -Force $Dest }
    Move-Item $inner[0].FullName $Dest
    Remove-Item -Force $zip
    Remove-Item -Recurse -Force $extract
}

if (-not (Test-Path $Gcc)) { throw "i686-w64-mingw32-gcc.exe missing after extract" }

$Bin = Join-Path $Dest "bin"
$Lib = Join-Path $Dest "i686-w64-mingw32\lib"
$Unwind = Join-Path $Lib "libunwind.a"
if (-not (Test-Path $Unwind)) { throw "libunwind.a missing: $Unwind" }

$anchorC = Join-Path $env:RUNNER_TEMP "pangyi-libgcc-anchor.c"
$anchorO = Join-Path $env:RUNNER_TEMP "pangyi-libgcc-anchor.o"
[System.IO.File]::WriteAllText($anchorC, "void pangyi_libgcc_anchor(void) {}`n")
& $Gcc -c -o $anchorO $anchorC
$Ar = Join-Path $Bin "llvm-ar.exe"
if (Test-Path (Join-Path $Lib "libgcc.a")) { Remove-Item -Force (Join-Path $Lib "libgcc.a") }
& $Ar rcs (Join-Path $Lib "libgcc.a") $anchorO
Copy-Item $Unwind (Join-Path $Lib "libgcc_eh.a") -Force
Copy-Item $Unwind (Join-Path $Lib "libgcc_s.a") -Force

if ($env:GITHUB_PATH) {
    Add-Content -Path $env:GITHUB_PATH -Value $Bin
}
Write-Host "llvm-mingw ready at $Dest"
& $Gcc --version
