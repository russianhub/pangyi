# 不联网，检查表情包脚本和仓库里的清单。
# 下载用假的 curl（环境变量 PANGYI_CURL），不访问 GitHub。
$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $false

$Repo = if ($env:GITHUB_WORKSPACE) { $env:GITHUB_WORKSPACE } else {
    (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
}
$Target = Join-Path $Repo "scripts\pangyi-packs.ps1"
if (-not (Test-Path -LiteralPath $Target)) { throw "missing $Target" }

function Count-DataLines {
    param([string]$Path)
    $utf8 = New-Object System.Text.UTF8Encoding $false
    $n = 0
    foreach ($line in [IO.File]::ReadAllLines($Path, $utf8)) {
        $t = $line.Trim()
        if ($t.Length -eq 0 -or $t.StartsWith("#")) { continue }
        $n++
    }
    return $n
}

# TSF 输入法 DLL 源码里不要出现联网 API。桌面主程序的账号模块是唯一例外：
# 它位于 bin/setup 子模块，只由 setup.exe 引用，不会编进 pangyi_ime.dll。
$accountDir = Join-Path $Repo "ime\src\bin\setup"
$src = Get-ChildItem -Path (Join-Path $Repo "ime\src") -Filter *.rs -Recurse |
    Where-Object { -not $_.FullName.StartsWith($accountDir, [StringComparison]::OrdinalIgnoreCase) }
$bad = Select-String -Path $src.FullName -Pattern 'WinHttp|InternetOpen|HttpClient|TcpStream|reqwest::|hyper::|ureq::'
if ($bad) {
    throw "IME source looks networked: $($bad | ForEach-Object { $_.Path + ':' + $_.LineNumber })"
}

$indexPath = Join-Path $Repo "packs\index.json"
$index = Get-Content -LiteralPath $indexPath -Raw -Encoding utf8 | ConvertFrom-Json
$packs = @($index.packs)
if ($packs.Count -lt 4) { throw "expected at least 4 official packs, got $($packs.Count)" }
$names = @{}
foreach ($p in $packs) {
    foreach ($field in @("name", "description", "filename", "sha256", "count", "license")) {
        if ($null -eq $p.$field -or "$($p.$field)" -eq "") { throw "index missing $field" }
    }
    if ($p.filename -ne "$($p.name).tsv") { throw "filename mismatch $($p.name)" }
    if ($names.ContainsKey([string]$p.name)) { throw "duplicate $($p.name)" }
    $names[[string]$p.name] = $true
    $path = Join-Path $Repo (Join-Path "packs" ([string]$p.filename))
    if (-not (Test-Path -LiteralPath $path)) { throw "missing $path" }
    $hash = (Get-FileHash -Algorithm SHA256 -LiteralPath $path).Hash.ToLowerInvariant()
    if ($hash -ne ([string]$p.sha256).ToLowerInvariant()) {
        throw "$($p.filename) sha256 $hash != $($p.sha256)"
    }
    $count = Count-DataLines $path
    if ($count -ne [int]$p.count) {
        throw "$($p.filename) count $count != $($p.count)"
    }
    $head = [IO.File]::ReadAllText($path, (New-Object System.Text.UTF8Encoding $false))
    if ($head -notmatch "CC BY-SA") { throw "$($p.filename) missing license" }
}
Write-Host "repo packs/index.json matches the files"

$root = Join-Path ([IO.Path]::GetTempPath()) ("pangyi-packs-test-" + [guid]::NewGuid().ToString("n"))
New-Item -ItemType Directory -Force -Path $root | Out-Null
$fixture = Join-Path $root "fixture"
$fakeDir = Join-Path $root "fake"
New-Item -ItemType Directory -Force -Path $fixture, $fakeDir | Out-Null

$utf8 = New-Object System.Text.UTF8Encoding $false
$packBody = "# 测试包`n# 许可 CC0`nxiao`tOK`tулыбка`nku`tNO`n"
[IO.File]::WriteAllText((Join-Path $fixture "emoji-common.tsv"), $packBody, $utf8)
[IO.File]::WriteAllText((Join-Path $fixture "a.tsv"), $packBody, $utf8)
$packHash = (Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $fixture "emoji-common.tsv")).Hash.ToLowerInvariant()
$packCount = Count-DataLines (Join-Path $fixture "emoji-common.tsv")

$indexObj = @{
    version = 1
    packs   = @(
        @{
            name        = "emoji-common"
            description = "测试用表情"
            filename    = "emoji-common.tsv"
            sha256      = $packHash
            count       = $packCount
            license     = "CC0-1.0"
            url         = "https://evil.example/steal.tsv"
        }
    )
}
$indexJson = $indexObj | ConvertTo-Json -Depth 6
[IO.File]::WriteAllText((Join-Path $fixture "index.json"), $indexJson + "`n", $utf8)

$badIndex = @{
    version = 1
    packs   = @(
        @{
            name        = "emoji-common"
            description = "坏校验"
            filename    = "emoji-common.tsv"
            sha256      = ("ab" * 32)
            count       = $packCount
            license     = "CC0-1.0"
        }
    )
} | ConvertTo-Json -Depth 6
[IO.File]::WriteAllText((Join-Path $fixture "bad-index.json"), $badIndex + "`n", $utf8)

$evilName = @{
    version = 1
    packs   = @(
        @{
            name        = "ok"
            description = "路径"
            filename    = "../x.tsv"
            sha256      = $packHash
            count       = 1
            license     = "CC0-1.0"
        }
    )
} | ConvertTo-Json -Depth 6
[IO.File]::WriteAllText((Join-Path $fixture "evil-name.json"), $evilName + "`n", $utf8)

$fake = @'
param()
$log = $env:PANGYI_CURL_LOG
$line = ($args -join "`n") + "`n----`n"
[IO.File]::AppendAllText($log, $line, (New-Object System.Text.UTF8Encoding $false))
$url = $null
$out = $null
for ($i = 0; $i -lt $args.Count; $i++) {
    if ($args[$i] -like "http://*" -or $args[$i] -like "https://*") { $url = $args[$i] }
    if ($args[$i] -eq "--output" -and ($i + 1) -lt $args.Count) { $out = $args[$i + 1] }
}
if (-not $url -or -not $out) { Write-Error "fake curl missing url or output"; exit 2 }
$leaf = ([Uri]$url).AbsolutePath.Split("/")[-1]
$src = Join-Path $env:PANGYI_CURL_FIXTURE $leaf
if (-not (Test-Path -LiteralPath $src)) { Write-Error "no fixture $leaf"; exit 22 }
Copy-Item -LiteralPath $src -Destination $out -Force
exit 0
'@
$fakePath = Join-Path $fakeDir "fake-curl.ps1"
[IO.File]::WriteAllText($fakePath, $fake, $utf8)

function Invoke-Packs {
    param(
        [string]$Label,
        [string[]]$ArgList,
        [bool]$ExpectSuccess,
        [string]$ExpectPattern,
        [string]$LogPath
    )
    if (Test-Path -LiteralPath $LogPath) { Remove-Item -LiteralPath $LogPath -Force }
    [IO.File]::WriteAllText($LogPath, "", $utf8)
    $saved = @{
        PANGYI_CURL         = $env:PANGYI_CURL
        PANGYI_CURL_LOG     = $env:PANGYI_CURL_LOG
        PANGYI_CURL_FIXTURE = $env:PANGYI_CURL_FIXTURE
    }
    try {
        $env:PANGYI_CURL = $fakePath
        $env:PANGYI_CURL_LOG = $LogPath
        $env:PANGYI_CURL_FIXTURE = $fixture
        $output = & pwsh -NoProfile -File $Target @ArgList 2>&1 | Out-String
        $code = $LASTEXITCODE
        Write-Host "---- $Label exit=$code ----"
        Write-Host $output
        if ($ExpectSuccess -and $code -ne 0) { throw "$Label`: exit $code, want 0`n$output" }
        if (-not $ExpectSuccess -and $code -eq 0) { throw "$Label`: exit 0, want non-zero`n$output" }
        if ($ExpectPattern -and $output -notmatch $ExpectPattern) {
            throw "$Label`: output did not match /$ExpectPattern/`n$output"
        }
        $global:LASTEXITCODE = 0
        return $output
    } finally {
        foreach ($k in $saved.Keys) {
            if ($null -eq $saved[$k] -or $saved[$k] -eq "") {
                Remove-Item "Env:$k" -ErrorAction SilentlyContinue
            } else {
                Set-Item "Env:$k" $saved[$k]
            }
        }
    }
}

$log = Join-Path $root "curl.log"
$dest = Join-Path $root "dest"
New-Item -ItemType Directory -Force -Path $dest | Out-Null
$official = "https://raw.githubusercontent.com/russianhub/pangyi/main/packs/index.json"

Invoke-Packs -Label "list-official" -ArgList @("-List") -ExpectSuccess $true -ExpectPattern "emoji-common" -LogPath $log | Out-Null
$seen = [IO.File]::ReadAllText($log, $utf8)
if ($seen -notmatch [regex]::Escape($official)) { throw "list did not request the official index:`n$seen" }
if ($seen -match "evil\.example") { throw "list followed a url inside the index:`n$seen" }
if ($seen -notmatch "(?m)^--max-redirs$" -or $seen -notmatch "(?m)^=https$") {
    throw "list did not lock curl to https without redirects:`n$seen"
}

Invoke-Packs -Label "list-http" -ArgList @("-List", "-IndexUrl", "http://example.com/index.json") -ExpectSuccess $false -ExpectPattern "必须是 https" -LogPath $log | Out-Null
$seen = [IO.File]::ReadAllText($log, $utf8)
if ($seen.Trim().Length -ne 0) { throw "http index must not call curl:`n$seen" }

Invoke-Packs -Label "install-one" -ArgList @("-Install", "-Name", "emoji-common", "-Dest", $dest) -ExpectSuccess $true -ExpectPattern "已安装 emoji-common.tsv" -LogPath $log | Out-Null
$seen = [IO.File]::ReadAllText($log, $utf8)
if ($seen -match "evil\.example") { throw "install followed index url field:`n$seen" }
$fileUrl = "https://raw.githubusercontent.com/russianhub/pangyi/main/packs/emoji-common.tsv"
if ($seen -notmatch [regex]::Escape($fileUrl)) { throw "install did not use the official file url:`n$seen" }
$installed = Join-Path $dest "emoji-common.tsv"
if (-not (Test-Path -LiteralPath $installed)) { throw "installed file missing" }
$got = (Get-FileHash -Algorithm SHA256 -LiteralPath $installed).Hash.ToLowerInvariant()
if ($got -ne $packHash) { throw "installed bytes changed" }

# 坏的校验不能留下文件。换一个目标目录，避免和上一份成功安装混在一起。
$destBad = Join-Path $root "dest-bad"
New-Item -ItemType Directory -Force -Path $destBad | Out-Null
# 显式清单的文件名必须是 index.json。临时换掉 fixture 里的 index.json。
$goodIndex = Join-Path $fixture "index.json"
$goodIndexSaved = Join-Path $fixture "index.good.json"
function Restore-GoodIndex {
    if (Test-Path -LiteralPath $goodIndex) { Remove-Item -LiteralPath $goodIndex -Force }
    if (Test-Path -LiteralPath $goodIndexSaved) {
        Move-Item -LiteralPath $goodIndexSaved -Destination $goodIndex
    }
}
Move-Item -LiteralPath $goodIndex -Destination $goodIndexSaved -Force
try {
    Copy-Item (Join-Path $fixture "bad-index.json") $goodIndex -Force
    Invoke-Packs -Label "bad-sha" -ArgList @(
        "-Install", "-Name", "emoji-common", "-Dest", $destBad,
        "-IndexUrl", "https://example.com/packs/index.json"
    ) -ExpectSuccess $false -ExpectPattern "SHA256 不一致" -LogPath $log | Out-Null
} finally {
    Restore-GoodIndex
}
if (Test-Path -LiteralPath (Join-Path $destBad "emoji-common.tsv")) {
    throw "hash mismatch must not leave a file"
}
$seen = [IO.File]::ReadAllText($log, $utf8)
if ($seen -notmatch "https://example.com/packs/emoji-common.tsv") {
    throw "explicit index should load the sibling file, not a url field:`n$seen"
}
if ($seen -match "evil\.example") { throw "explicit index still followed url field:`n$seen" }

Invoke-Packs -Label "explicit-url" -ArgList @(
    "-Install", "-Name", "mypack",
    "-Url", "https://example.com/files/a.tsv",
    "-Sha256", $packHash,
    "-Dest", $dest
) -ExpectSuccess $true -ExpectPattern "已安装 mypack.tsv" -LogPath $log | Out-Null
if (-not (Test-Path -LiteralPath (Join-Path $dest "mypack.tsv"))) { throw "explicit url did not save" }
$seen = [IO.File]::ReadAllText($log, $utf8)
if ($seen -notmatch "https://example.com/files/a.tsv") { throw "explicit url was rewritten:`n$seen" }

Invoke-Packs -Label "explicit-http" -ArgList @(
    "-Install", "-Name", "mypack",
    "-Url", "http://evil.example/a.tsv",
    "-Sha256", $packHash,
    "-Dest", $dest
) -ExpectSuccess $false -ExpectPattern "必须是 https" -LogPath $log | Out-Null
$seen = [IO.File]::ReadAllText($log, $utf8)
if ($seen.Trim().Length -ne 0) { throw "http file url must not call curl:`n$seen" }

Invoke-Packs -Label "bad-name" -ArgList @("-Install", "-Name", "../x", "-Dest", $dest) -ExpectSuccess $false -ExpectPattern "不合法" -LogPath $log | Out-Null
$seen = [IO.File]::ReadAllText($log, $utf8)
if ($seen.Trim().Length -ne 0) { throw "bad name must not call curl:`n$seen" }

Move-Item -LiteralPath $goodIndex -Destination $goodIndexSaved -Force
try {
    Copy-Item (Join-Path $fixture "evil-name.json") $goodIndex -Force
    Invoke-Packs -Label "evil-filename" -ArgList @(
        "-Install", "-All", "-Dest", $dest,
        "-IndexUrl", "https://example.com/packs/index.json"
    ) -ExpectSuccess $false -ExpectPattern "文件名必须是" -LogPath $log | Out-Null
} finally {
    Restore-GoodIndex
}

$custom = Join-Path $dest "my-own.tsv"
[IO.File]::WriteAllText($custom, "xiao`tKEEP`n", $utf8)
Invoke-Packs -Label "remove-one" -ArgList @("-Remove", "-Name", "emoji-common", "-Dest", $dest) -ExpectSuccess $true -ExpectPattern "已删除 emoji-common.tsv" -LogPath $log | Out-Null
if (Test-Path -LiteralPath $installed) { throw "remove -Name left the file" }
if (-not (Test-Path -LiteralPath $custom)) { throw "remove -Name deleted a different file" }
$seen = [IO.File]::ReadAllText($log, $utf8)
if ($seen.Trim().Length -ne 0) { throw "remove -Name must not download:`n$seen" }

# 装回官方测试包，再 -Remove -All。自定义文件要留下。
Invoke-Packs -Label "reinstall" -ArgList @("-Install", "-Name", "emoji-common", "-Dest", $dest) -ExpectSuccess $true -ExpectPattern "已安装" -LogPath $log | Out-Null
Invoke-Packs -Label "remove-all" -ArgList @("-Remove", "-All", "-Dest", $dest) -ExpectSuccess $true -ExpectPattern "已删除 emoji-common.tsv" -LogPath $log | Out-Null
if (Test-Path -LiteralPath (Join-Path $dest "emoji-common.tsv")) { throw "remove -All left an official file" }
if (-not (Test-Path -LiteralPath $custom)) { throw "remove -All deleted a custom pack" }
if (-not (Test-Path -LiteralPath (Join-Path $dest "mypack.tsv"))) { throw "remove -All deleted an explicit install that is not in the index" }

Write-Host "pangyi-packs tests passed"
$global:LASTEXITCODE = 0
exit 0
