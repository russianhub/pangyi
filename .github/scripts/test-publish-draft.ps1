# 用和 GitHub Actions 相同的方式跑 publish-draft.ps1 -CheckOnly：
#   pwsh -Command ". '脚本' -CheckOnly"
# 不调用真正的 gh，也不创建 Release。
# 三种结果：没有 Release 时进程退出码必须是 0；已有 Release、缺少说明文件时必须非 0。
$ErrorActionPreference = "Stop"
# 子进程按设计会以非 0 退出。这里自己看 $LASTEXITCODE，不要让本机命令失败直接中断。
$PSNativeCommandUseErrorActionPreference = $false

$Repo = if ($env:GITHUB_WORKSPACE) { $env:GITHUB_WORKSPACE } else {
    (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
}
$Target = Join-Path $Repo ".github\scripts\publish-draft.ps1"
if (-not (Test-Path -LiteralPath $Target)) { throw "missing $Target" }

$root = Join-Path ([System.IO.Path]::GetTempPath()) ("pangyi-publish-check-" + [guid]::NewGuid().ToString("n"))
New-Item -ItemType Directory -Force -Path $root | Out-Null

function New-FakeGh {
    param([string]$Dir)
    New-Item -ItemType Directory -Force -Path $Dir | Out-Null
    $lines = @(
        '@echo off',
        'if "%PANGYI_FAKE_GH%"=="exists" (',
        '  echo {"tagName":"v9.9.9","isDraft":false,"url":"https://example.invalid/release"}',
        '  exit /b 0',
        ')',
        'echo release not found 1>&2',
        'exit /b 1'
    )
    $path = Join-Path $Dir "gh.cmd"
    $text = ($lines -join "`r`n") + "`r`n"
    [System.IO.File]::WriteAllText($path, $text, [System.Text.ASCIIEncoding]::new())
}

function New-Workspace {
    param([string]$Dir, [switch]$WithNotes)
    New-Item -ItemType Directory -Force -Path $Dir | Out-Null
    if ($WithNotes) {
        $notesDir = Join-Path $Dir "docs\releases"
        New-Item -ItemType Directory -Force -Path $notesDir | Out-Null
        [System.IO.File]::WriteAllText((Join-Path $notesDir "v9.9.9.md"), "ci notes`n")
    }
}

function Invoke-CheckOnly {
    param(
        [string]$Label,
        [string]$Workspace,
        [string]$FakeDir,
        [string]$FakeMode,
        [bool]$ExpectSuccess,
        [string]$ExpectPattern
    )
    $savedPath = $env:PATH
    $keys = @("RELEASE_TAG", "GH_TOKEN", "GITHUB_REF_TYPE", "GITHUB_WORKSPACE", "GITHUB_REPOSITORY", "PANGYI_FAKE_GH")
    $saved = @{}
    foreach ($k in $keys) { $saved[$k] = [Environment]::GetEnvironmentVariable($k) }
    try {
        $env:PATH = "$FakeDir;$savedPath"
        $env:RELEASE_TAG = "v9.9.9"
        $env:GH_TOKEN = "ci-dummy-not-a-real-token"
        $env:GITHUB_REF_TYPE = "tag"
        $env:GITHUB_WORKSPACE = $Workspace
        $env:GITHUB_REPOSITORY = "russianhub/pangyi"
        if ($FakeMode -eq "exists") {
            $env:PANGYI_FAKE_GH = "exists"
        } else {
            Remove-Item Env:PANGYI_FAKE_GH -ErrorAction SilentlyContinue
        }
        $resolved = Get-Command gh -ErrorAction SilentlyContinue
        Write-Host "gh for ${Label}: $($resolved.Source)"
        if (-not $resolved -or $resolved.Source -notlike "$FakeDir*") {
            throw "$Label`: gh did not resolve to the fake under $FakeDir (got $($resolved.Source))"
        }
        $quoted = $Target.Replace("'", "''")
        # 与 actions/runner 的 pwsh shell 一致：-Command ". '{0}'"
        $command = ". '$quoted' -CheckOnly"
        $output = & pwsh -NoProfile -Command $command 2>&1 | Out-String
        $code = $LASTEXITCODE
        Write-Host "---- $Label exit=$code ----"
        Write-Host $output
        if ($ExpectSuccess -and $code -ne 0) {
            throw "$Label`: exit $code, want 0"
        }
        if (-not $ExpectSuccess -and $code -eq 0) {
            throw "$Label`: exit 0, want non-zero"
        }
        if ($output -notmatch $ExpectPattern) {
            throw "$Label`: output did not match /$ExpectPattern/"
        }
        if ($ExpectSuccess -and $output -match "draft release created") {
            throw "$Label`: CheckOnly must not create a release"
        }
    } finally {
        $env:PATH = $savedPath
        foreach ($k in $keys) {
            if ($null -eq $saved[$k] -or $saved[$k] -eq "") {
                Remove-Item "Env:$k" -ErrorAction SilentlyContinue
            } else {
                Set-Item "Env:$k" $saved[$k]
            }
        }
    }
}

$fake = Join-Path $root "fake-gh"
New-FakeGh $fake
$withNotes = Join-Path $root "with-notes"
$noNotes = Join-Path $root "no-notes"
New-Workspace $withNotes -WithNotes
New-Workspace $noNotes

Invoke-CheckOnly -Label "absent" -Workspace $withNotes -FakeDir $fake -FakeMode "absent" -ExpectSuccess $true -ExpectPattern "no existing release for v9\.9\.9"
Invoke-CheckOnly -Label "exists" -Workspace $withNotes -FakeDir $fake -FakeMode "exists" -ExpectSuccess $false -ExpectPattern "already exists"
Invoke-CheckOnly -Label "missing-notes" -Workspace $noNotes -FakeDir $fake -FakeMode "absent" -ExpectSuccess $false -ExpectPattern "release notes missing"

Write-Host "publish-draft -CheckOnly cases passed"
