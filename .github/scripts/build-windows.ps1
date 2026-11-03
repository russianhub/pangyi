# 在 windows-latest 上构建旁译：cargo test、64 位 DLL、32 位 DLL、安装包。
# 不上传 Release，也不把 payload / stub 对象提交进仓库。
$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true

$Repo = if ($env:GITHUB_WORKSPACE) { $env:GITHUB_WORKSPACE } else { (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path }
Set-Location $Repo

$MingwBin = Join-Path $Repo "tools\llvm-mingw\bin"
$Gcc = Join-Path $MingwBin "i686-w64-mingw32-gcc.exe"
if (-not (Test-Path $Gcc)) { throw "llvm-mingw is not set up ($Gcc)" }
$env:PATH = "$MingwBin;$env:PATH"

function Assert-MinBytes {
    param([string]$Path, [Int64]$Min, [string]$Label)
    $n = (Get-Item $Path).Length
    Write-Host "$Label size=$n"
    if ($n -lt $Min) { throw "$Label is $n bytes, want at least $Min" }
}

function Get-PeMachine {
    param([string]$Path)
    $fs = [System.IO.File]::OpenRead($Path)
    try {
        $br = New-Object System.IO.BinaryReader($fs)
        $mz = $br.ReadUInt16()
        if ($mz -ne 0x5A4D) { throw "not MZ: $Path" }
        $fs.Position = 0x3C
        $peOff = $br.ReadInt32()
        if ($peOff -le 0) { throw "bad e_lfanew in $Path" }
        $fs.Position = $peOff
        $sig = $br.ReadUInt32()
        if ($sig -ne 0x00004550) { throw "not PE: $Path" }
        return [int]$br.ReadUInt16()
    } finally {
        $fs.Dispose()
    }
}

function Assert-Pe {
    param([string]$Path, [int]$Expected, [string]$Label)
    $machine = Get-PeMachine $Path
    $size = (Get-Item $Path).Length
    Write-Host ("{0}: machine=0x{1:X4} size={2}" -f $Label, $machine, $size)
    if ($machine -ne $Expected) {
        throw ("{0} machine 0x{1:X4} != 0x{2:X4}" -f $Label, $machine, $Expected)
    }
}

function Assert-Ascii {
    param([string]$Path, [string]$Text)
    # findstr 会把含 NUL 的 PE 当成二进制跳过，所以用 Python 做字节搜索。
    $py = @"
import pathlib, sys
data = pathlib.Path(sys.argv[1]).read_bytes()
sys.exit(0 if sys.argv[2].encode('ascii') in data else 1)
"@
    $tmp = Join-Path $env:RUNNER_TEMP "pangyi-assert-ascii.py"
    [System.IO.File]::WriteAllText($tmp, $py)
    $prev = $PSNativeCommandUseErrorActionPreference
    $PSNativeCommandUseErrorActionPreference = $false
    if (Get-Command py -ErrorAction SilentlyContinue) {
        & py -3 $tmp $Path $Text
    } else {
        & python $tmp $Path $Text
    }
    $code = $LASTEXITCODE
    $PSNativeCommandUseErrorActionPreference = $prev
    if ($code -ne 0) { throw "missing '$Text' in $Path" }
}

function Get-ImportDlls {
    param([string]$Path)
    $objdump = Join-Path $MingwBin "llvm-objdump.exe"
    $prev = $PSNativeCommandUseErrorActionPreference
    $PSNativeCommandUseErrorActionPreference = $false
    $lines = & $objdump -p $Path
    $code = $LASTEXITCODE
    $PSNativeCommandUseErrorActionPreference = $prev
    if ($code -ne 0) { throw "llvm-objdump failed for $Path" }
    $names = @()
    foreach ($line in $lines) {
        if ($line -match "DLL Name:\s*(.+)$") {
            $names += $Matches[1].Trim()
        }
    }
    if ($names.Count -eq 0) {
        $sample = ($lines | Select-Object -First 80) -join "`n"
        throw "no DLL imports parsed from $Path`n$sample"
    }
    return $names
}

function Invoke-Python {
    param([string[]]$ArgList)
    if (Get-Command py -ErrorAction SilentlyContinue) {
        & py -3 @ArgList
    } else {
        & python @ArgList
    }
}

# setup.rs 用 include_bytes! 引用 payload。测试只需要文件存在，不能把真正的 DLL 编进测试二进制。
$payload = Join-Path $Repo "ime\payload"
New-Item -ItemType Directory -Force -Path $payload | Out-Null
$marker = [Text.Encoding]::ASCII.GetBytes("pangyi-ci-placeholder")
foreach ($name in @("pangyi_ime.dll", "pangyi_ime-x86.dll", "pangyi.ico")) {
    [System.IO.File]::WriteAllBytes((Join-Path $payload $name), $marker)
}

Write-Host "rustc $(rustc --version)"
cargo test --locked --manifest-path ime/Cargo.toml

# DLL 构建时不要留着占位图标，让 build.rs 走「图标不存在就跳过」那条路。
Remove-Item -Recurse -Force $payload

Write-Host "building x64 DLL"
cargo build --release --lib --locked --manifest-path ime/Cargo.toml
$dll64 = Join-Path $Repo "ime\target\release\pangyi_ime.dll"
Assert-MinBytes $dll64 20000000 "x64 dll"
Assert-Pe $dll64 0x8664 "x64 dll"
Assert-Ascii $dll64 "DllGetClassObject"

$stubC = Join-Path $Repo "ime\build\x86-stub.c"
$stubO = Join-Path $Repo "ime\build\x86-stub.o"
Write-Host "assembling x86 stub"
& $Gcc -c -O2 -o $stubO $stubC
& (Join-Path $MingwBin "llvm-nm.exe") $stubO

$stubArg = (Resolve-Path $stubO).Path.Replace("\", "/")
$gccArg = (Resolve-Path $Gcc).Path.Replace("\", "/")
$libArg = (Resolve-Path (Join-Path $Repo "tools\llvm-mingw\i686-w64-mingw32\lib")).Path.Replace("\", "/")
# 只作用于这一步。64 位构建和安装包构建不继承这些标志。
$env:RUSTFLAGS = @(
    "-C", "linker=$gccArg",
    "-C", "link-self-contained=no",
    "-C", "target-feature=+crt-static",
    "-C", "link-arg=-L$libArg",
    "-C", "link-arg=$stubArg",
    "-C", "link-arg=-Wl,--allow-multiple-definition"
) -join " "
Write-Host "RUSTFLAGS=$env:RUSTFLAGS"
Write-Host "building x86 DLL"
cargo build --release --lib --locked --manifest-path ime/Cargo.toml --target i686-pc-windows-gnu
Remove-Item Env:RUSTFLAGS

$dll32 = Join-Path $Repo "ime\target\i686-pc-windows-gnu\release\pangyi_ime.dll"
Assert-MinBytes $dll32 20000000 "x86 dll"
Assert-Pe $dll32 0x014C "x86 dll"
Assert-Ascii $dll32 "DllGetClassObject"
$imports = Get-ImportDlls $dll32
Write-Host ("x86 imports: " + ($imports -join ", "))
foreach ($bad in @("libunwind.dll", "libgcc_s_dw2-1.dll", "libgcc_s_seh-1.dll")) {
    foreach ($name in $imports) {
        if ($name -ieq $bad) { throw "x86 DLL imports $name" }
    }
}
$hasMsvcrt = $false
foreach ($name in $imports) {
    if ($name -ieq "msvcrt.dll") { $hasMsvcrt = $true }
}
if (-not $hasMsvcrt) { throw "x86 DLL does not import msvcrt.dll: $($imports -join ', ')" }

New-Item -ItemType Directory -Force -Path $payload | Out-Null
Copy-Item $dll64 (Join-Path $payload "pangyi_ime.dll") -Force
Copy-Item $dll32 (Join-Path $payload "pangyi_ime-x86.dll") -Force
Invoke-Python @(
    (Join-Path $Repo "tools\make_ico.py"),
    (Join-Path $Repo "ime\build\icon"),
    (Join-Path $payload "pangyi.ico")
)
Assert-MinBytes (Join-Path $payload "pangyi.ico") 1000 "pangyi.ico"

Write-Host "building setup installer"
cargo build --release --bin setup --locked --manifest-path ime/Cargo.toml
$setup = Join-Path $Repo "ime\target\release\setup.exe"
Assert-MinBytes $setup 40000000 "setup.exe"
Assert-Pe $setup 0x8664 "setup.exe"

if ($env:GITHUB_STEP_SUMMARY) {
    $x64n = (Get-Item $dll64).Length
    $x86n = (Get-Item $dll32).Length
    $setupn = (Get-Item $setup).Length
    @"
### Windows build

| artifact | bytes |
| --- | ---: |
| pangyi_ime.dll (x64) | $x64n |
| pangyi_ime.dll (i686) | $x86n |
| setup.exe | $setupn |
"@ | Out-File -FilePath $env:GITHUB_STEP_SUMMARY -Append -Encoding utf8
}
