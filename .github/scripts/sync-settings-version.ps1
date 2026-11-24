# 把 settings/Cargo.toml 的包版本，以及 settings/Cargo.lock 里
# pangyi-settings 那一项的版本，改成标签上的版本号。
# 只改行首的 version，以及 lock 里这个包名的下一行。依赖的 version 不动。
# 规则与 ime/src/bin/setup_plan.rs 的 rewrite_manifest_version /
# rewrite_lock_package_version 一致。--locked 构建要求两边版本相同。
# 不改 ime、updater、account，也不改任何 git 标签或已有 Release。
param(
    [Parameter(Mandatory = $true)]
    [string]$Version
)

$ErrorActionPreference = "Stop"

if ($Version -notmatch '^\d+\.\d+\.\d+$') {
    throw "refusing settings version (want major.minor.patch): $Version"
}

$Repo = if ($env:GITHUB_WORKSPACE) { $env:GITHUB_WORKSPACE } else {
    (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
}
$tomlPath = Join-Path $Repo "settings\Cargo.toml"
$lockPath = Join-Path $Repo "settings\Cargo.lock"
if (-not (Test-Path -LiteralPath $tomlPath)) { throw "missing $tomlPath" }
if (-not (Test-Path -LiteralPath $lockPath)) { throw "missing $lockPath" }

$toml = [System.IO.File]::ReadAllText($tomlPath)
$leading = [regex]::Matches($toml, '(?m)^version = .*$')
if ($leading.Count -ne 1) {
    throw "settings/Cargo.toml should have exactly one leading version line, got $($leading.Count)"
}
$toml2 = [regex]::Replace($toml, '(?m)^version = .*$', ('version = "' + $Version + '"'))
$utf8 = New-Object System.Text.UTF8Encoding $false
[System.IO.File]::WriteAllText($tomlPath, $toml2, $utf8)

$lock = [System.IO.File]::ReadAllText($lockPath)
$needle = 'name = "pangyi-settings"'
$idx = $lock.IndexOf($needle)
if ($idx -lt 0) { throw "pangyi-settings package is missing from settings/Cargo.lock" }
if ($lock.IndexOf($needle, $idx + $needle.Length) -ge 0) {
    throw "settings/Cargo.lock has more than one pangyi-settings package"
}
$after = $idx + $needle.Length
$nlLen = 0
if ($after + 1 -lt $lock.Length -and $lock.Substring($after, 2) -eq "`r`n") {
    $nlLen = 2
} elseif ($after -lt $lock.Length -and $lock[$after] -eq "`n") {
    $nlLen = 1
} else {
    throw "expected a newline after the pangyi-settings package name"
}
$verAt = $after + $nlLen
$prefix = 'version = "'
if (-not $lock.Substring($verAt).StartsWith($prefix)) {
    throw "expected a version line under pangyi-settings"
}
$valueStart = $verAt + $prefix.Length
$endQuote = $lock.IndexOf('"', $valueStart)
if ($endQuote -lt 0) { throw "unterminated pangyi-settings version in Cargo.lock" }
$old = $lock.Substring($valueStart, $endQuote - $valueStart)
$lock2 = $lock.Substring(0, $valueStart) + $Version + $lock.Substring($endQuote)
[System.IO.File]::WriteAllText($lockPath, $lock2, $utf8)

Write-Host "settings/Cargo.toml version=$Version"
Write-Host "settings/Cargo.lock pangyi-settings $old -> $Version"
