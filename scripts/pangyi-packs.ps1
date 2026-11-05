# 下载、列出、删除旁译表情包。
# 输入法 DLL 不联网。只有运行这个脚本时才会下载。
#
# 默认只访问：
#   https://raw.githubusercontent.com/russianhub/pangyi/<Ref>/packs/
# 不跟随重定向。清单里如果写了别的网址，一律不用，只按文件名拼到清单所在目录。
#
# 你用 -IndexUrl 或 -Url 明确写出的地址可以是别的主机，但必须是 https，也不能带账号密码。
# 环境变量 PANGYI_CURL 只给测试用：把它指到一个假的下载脚本。平时不要设置。
param(
    [switch]$List,
    [switch]$Install,
    [switch]$Remove,
    [switch]$All,
    [string]$Name = "",
    [string]$IndexUrl = "",
    [string]$Url = "",
    [string]$Sha256 = "",
    [string]$Dest = "",
    [string]$Ref = "main"
)

$ErrorActionPreference = "Stop"
$PSNativeCommandUseErrorActionPreference = $true

function Assert-SafeName {
    param([string]$Value, [string]$What)
    if ($Value -notmatch '^[a-z0-9][a-z0-9-]{0,40}$') {
        throw "$What 不合法（只用小写字母、数字和短横线）：$Value"
    }
}

function Test-HttpsUrl {
    param([string]$Raw, [string]$What)
    if ([string]::IsNullOrWhiteSpace($Raw)) {
        throw "$What 是空的"
    }
    if ($Raw -match '[\s\x00-\x1f\\]') {
        throw "$What 含有空白或反斜杠"
    }
    $u = $null
    if (-not [Uri]::TryCreate($Raw, [UriKind]::Absolute, [ref]$u)) {
        throw "$What 不是网址：$Raw"
    }
    if ($u.Scheme -ne "https") {
        throw "$What 必须是 https：$Raw"
    }
    if ($u.UserInfo) {
        throw "$What 不能带账号密码"
    }
    if ([string]::IsNullOrWhiteSpace($u.Host)) {
        throw "$What 没有主机名"
    }
    if ($u.Query -or $u.Fragment) {
        throw "$What 不能带查询参数或片段"
    }
    return $u
}

function Test-OfficialGithubRaw {
    param([string]$Raw)
    $u = Test-HttpsUrl $Raw "官方网址"
    if ($u.Host -ne "raw.githubusercontent.com") {
        throw "只允许 raw.githubusercontent.com/russianhub/pangyi。别的网址请用 -IndexUrl 或 -Url 显式指定"
    }
    if ($u.AbsolutePath -notmatch '^/russianhub/pangyi/[A-Za-z0-9._-]+/packs/[A-Za-z0-9][A-Za-z0-9._-]{0,80}\.(tsv|json)$') {
        throw "只允许 raw.githubusercontent.com/russianhub/pangyi 的 packs 目录。别的网址请用 -IndexUrl 或 -Url 显式指定"
    }
}

function Get-IndexUrl {
    if ($Ref -notmatch '^[A-Za-z0-9._-]{1,64}$') {
        throw "Ref 只能含字母、数字、点、下划线和短横线"
    }
    if ($IndexUrl) {
        $u = Test-HttpsUrl $IndexUrl "清单网址"
        if ($u.AbsolutePath -notmatch '/index\.json$') {
            throw "清单网址要以 /index.json 结尾"
        }
        return $IndexUrl
    }
    $official = "https://raw.githubusercontent.com/russianhub/pangyi/$Ref/packs/index.json"
    Test-OfficialGithubRaw $official
    return $official
}

function Get-SiblingUrl {
    param([string]$Index, [string]$Filename)
    if ($Filename -notmatch '^[a-z0-9][a-z0-9-]{0,40}\.tsv$') {
        throw "文件名不合法：$Filename"
    }
    $u = Test-HttpsUrl $Index "清单网址"
    $slash = $u.AbsolutePath.LastIndexOf("/")
    if ($slash -lt 1) {
        throw "清单网址没有目录"
    }
    $dir = $u.AbsolutePath.Substring(0, $slash)
    $built = "{0}://{1}{2}/{3}" -f $u.Scheme, $u.Authority, $dir, $Filename
    $fileUri = Test-HttpsUrl $built "包文件网址"
    if ($fileUri.Host -ne $u.Host) {
        throw "包文件和清单不在同一台主机上"
    }
    if ($u.Host -eq "raw.githubusercontent.com" -and $u.AbsolutePath -match '^/russianhub/pangyi/') {
        Test-OfficialGithubRaw $built
    }
    return $built
}

function Invoke-CurlDownload {
    param([string]$From, [string]$OutFile)
    # 再查一次。调用方已经查过；这里挡住以后改漏的路径。
    Test-HttpsUrl $From "下载网址" | Out-Null
    $curlArgs = @(
        "--proto", "=https",
        "--proto-redir", "=https",
        "--max-redirs", "0",
        "--fail",
        "--silent",
        "--show-error",
        "--output", $OutFile,
        $From
    )
    $prev = $PSNativeCommandUseErrorActionPreference
    $PSNativeCommandUseErrorActionPreference = $false
    try {
        if ($env:PANGYI_CURL) {
            & pwsh -NoProfile -File $env:PANGYI_CURL @curlArgs
        } else {
            & curl.exe @curlArgs
        }
        $code = $LASTEXITCODE
    } finally {
        $PSNativeCommandUseErrorActionPreference = $prev
    }
    if ($code -ne 0) {
        throw "下载失败（退出码 $code）：$From"
    }
    $global:LASTEXITCODE = 0
}

function Read-PackIndex {
    param([string]$JsonText)
    $obj = $JsonText | ConvertFrom-Json
    if ($null -eq $obj.packs) {
        throw "清单里没有 packs"
    }
    $out = @()
    $seen = @{}
    foreach ($p in @($obj.packs)) {
        foreach ($field in @("name", "description", "filename", "sha256", "count", "license")) {
            $value = $p.$field
            if ($null -eq $value -or "$value" -eq "") {
                throw "清单条目缺少 $field"
            }
        }
        $name = [string]$p.name
        $file = [string]$p.filename
        Assert-SafeName $name "包名"
        if ($file -ne "$name.tsv") {
            throw "文件名必须是 $name.tsv，实际是 $file"
        }
        if ($seen.ContainsKey($name)) {
            throw "清单里包名重复：$name"
        }
        $seen[$name] = $true
        $sha = ([string]$p.sha256).ToLowerInvariant()
        if ($sha -notmatch '^[0-9a-f]{64}$') {
            throw "sha256 不合法：$name"
        }
        $count = 0
        if (-not [int]::TryParse([string]$p.count, [ref]$count) -or $count -lt 0) {
            throw "条数不合法：$name"
        }
        $out += [pscustomobject]@{
            Name        = $name
            Description = [string]$p.description
            Filename    = $file
            Sha256      = $sha
            Count       = $count
            License     = [string]$p.license
        }
    }
    if ($out.Count -lt 1) {
        throw "清单是空的"
    }
    return $out
}

function Get-IndexPacks {
    $index = Get-IndexUrl
    $tmp = Join-Path ([IO.Path]::GetTempPath()) ("pangyi-index-" + [guid]::NewGuid().ToString("n") + ".json")
    try {
        Invoke-CurlDownload -From $index -OutFile $tmp
        $utf8 = New-Object System.Text.UTF8Encoding $false
        $text = [IO.File]::ReadAllText($tmp, $utf8)
    } finally {
        if (Test-Path -LiteralPath $tmp) {
            Remove-Item -LiteralPath $tmp -Force
        }
    }
    return Read-PackIndex $text
}

function Get-DataLineCount {
    param([string]$Path)
    $utf8 = New-Object System.Text.UTF8Encoding $false
    $n = 0
    foreach ($line in [IO.File]::ReadAllLines($Path, $utf8)) {
        $t = $line.Trim()
        if ($t.Length -eq 0 -or $t.StartsWith("#")) {
            continue
        }
        $n++
    }
    return $n
}

function Get-DestDir {
    if ($Dest) {
        return $Dest
    }
    if (-not $env:LOCALAPPDATA) {
        throw "没有 LOCALAPPDATA。请用 -Dest 指定目录"
    }
    return (Join-Path $env:LOCALAPPDATA "Pangyi\packs")
}

function Resolve-PackPath {
    param([string]$Dir, [string]$Filename)
    if ($Filename -notmatch '^[a-z0-9][a-z0-9-]{0,40}\.tsv$') {
        throw "文件名不合法：$Filename"
    }
    if (-not (Test-Path -LiteralPath $Dir)) {
        New-Item -ItemType Directory -Force -Path $Dir | Out-Null
    }
    $root = [IO.Path]::GetFullPath($Dir)
    $sep = [IO.Path]::DirectorySeparatorChar
    if (-not $root.EndsWith($sep) -and -not $root.EndsWith("/")) {
        $root += $sep
    }
    $full = [IO.Path]::GetFullPath((Join-Path $Dir $Filename))
    if (-not $full.StartsWith($root, [StringComparison]::OrdinalIgnoreCase)) {
        throw "拒绝写到目录外面：$full"
    }
    return $full
}

function Install-CheckedFile {
    param(
        [string]$From,
        [string]$ExpectedSha,
        [string]$SaveName,
        [int]$ExpectedCount = -1
    )
    Assert-SafeName $SaveName "包名"
    $sha = $ExpectedSha.ToLowerInvariant()
    if ($sha -notmatch '^[0-9a-f]{64}$') {
        throw "sha256 不合法"
    }
    $tmp = Join-Path ([IO.Path]::GetTempPath()) ("pangyi-pack-" + [guid]::NewGuid().ToString("n") + ".tsv")
    $kept = $false
    try {
        Invoke-CurlDownload -From $From -OutFile $tmp
        $got = (Get-FileHash -Algorithm SHA256 -LiteralPath $tmp).Hash.ToLowerInvariant()
        if ($got -ne $sha) {
            throw "SHA256 不一致：$SaveName`n期望 $sha`n实际 $got"
        }
        if ($ExpectedCount -ge 0) {
            $n = Get-DataLineCount $tmp
            if ($n -ne $ExpectedCount) {
                throw "条数不一致：$SaveName 清单写 $ExpectedCount，文件里是 $n"
            }
        }
        $target = Resolve-PackPath (Get-DestDir) "$SaveName.tsv"
        Move-Item -LiteralPath $tmp -Destination $target -Force
        $kept = $true
        Write-Output "已安装 $SaveName.tsv"
        Write-Output $target
    } finally {
        if (-not $kept -and (Test-Path -LiteralPath $tmp)) {
            Remove-Item -LiteralPath $tmp -Force
        }
    }
}

function Show-Pack {
    param($Pack)
    Write-Output ("{0}`t{1} 条`t{2}" -f $Pack.Name, $Pack.Count, $Pack.License)
    Write-Output ("  {0}" -f $Pack.Description)
    Write-Output ("  {0}" -f $Pack.Filename)
    Write-Output ("  {0}" -f $Pack.Sha256)
}

$actions = 0
if ($List) { $actions++ }
if ($Install) { $actions++ }
if ($Remove) { $actions++ }
if ($actions -ne 1) {
    throw "请只选一个：-List、-Install 或 -Remove"
}
if ($Url -and -not $Install) {
    throw "-Url 只能和 -Install 一起用"
}
if ($Sha256 -and -not $Url) {
    throw "-Sha256 只能和 -Url 一起用"
}
if ($All -and $Name) {
    throw "-All 和 -Name 不要一起用"
}
if ($Url -and $IndexUrl) {
    throw "-Url 和 -IndexUrl 不要一起用"
}

if ($List) {
    if ($All -or $Name -or $Url -or $Sha256) {
        throw "-List 只列出清单，不要带 -All、-Name、-Url 或 -Sha256"
    }
    foreach ($p in @(Get-IndexPacks)) {
        Show-Pack $p
    }
    $global:LASTEXITCODE = 0
    exit 0
}

if ($Install -and $Url) {
    if ($All -or -not $Name) {
        throw "-Url 需要 -Name，用来决定保存的文件名，并且不能和 -All 一起用"
    }
    if (-not $Sha256) {
        throw "-Url 需要 -Sha256"
    }
    Assert-SafeName $Name "包名"
    Test-HttpsUrl $Url "网址" | Out-Null
    Install-CheckedFile -From $Url -ExpectedSha $Sha256 -SaveName $Name
    $global:LASTEXITCODE = 0
    exit 0
}

if ($Install) {
    if (-not $All -and -not $Name) {
        throw "请指定 -Name <包名> 或 -All"
    }
    if ($Name) {
        Assert-SafeName $Name "包名"
    }
    $packs = @(Get-IndexPacks)
    $want = @()
    if ($All) {
        $want = $packs
    } else {
        Assert-SafeName $Name "包名"
        $hit = $packs | Where-Object { $_.Name -eq $Name }
        if (-not $hit) {
            throw "清单里没有这个包：$Name"
        }
        $want = @($hit)
    }
    $index = Get-IndexUrl
    foreach ($p in $want) {
        $from = Get-SiblingUrl -Index $index -Filename $p.Filename
        Install-CheckedFile -From $from -ExpectedSha $p.Sha256 -SaveName $p.Name -ExpectedCount $p.Count
    }
    $global:LASTEXITCODE = 0
    exit 0
}

# -Remove
if ($All) {
    $dir = Get-DestDir
    foreach ($p in @(Get-IndexPacks)) {
        if (-not (Test-Path -LiteralPath $dir)) {
            Write-Output "本来就没有 $($p.Filename)"
            continue
        }
        $path = Join-Path $dir $p.Filename
        if (Test-Path -LiteralPath $path) {
            Remove-Item -LiteralPath $path -Force
            Write-Output "已删除 $($p.Filename)"
        } else {
            Write-Output "本来就没有 $($p.Filename)"
        }
    }
    $global:LASTEXITCODE = 0
    exit 0
}
if (-not $Name) {
    throw "请指定 -Name <包名> 或 -All"
}
if ($IndexUrl) {
    throw "-Remove -Name 不读清单。-IndexUrl 请和 -Remove -All 一起用"
}
Assert-SafeName $Name "包名"
$dir = Get-DestDir
$path = Join-Path $dir "$Name.tsv"
if (Test-Path -LiteralPath $dir) {
    $full = [IO.Path]::GetFullPath($path)
    $root = [IO.Path]::GetFullPath($dir)
    $sep = [IO.Path]::DirectorySeparatorChar
    if (-not $root.EndsWith($sep) -and -not $root.EndsWith("/")) { $root += $sep }
    if (-not $full.StartsWith($root, [StringComparison]::OrdinalIgnoreCase)) {
        throw "拒绝删到目录外面：$full"
    }
}
if (Test-Path -LiteralPath $path) {
    Remove-Item -LiteralPath $path -Force
    Write-Output "已删除 $Name.tsv"
} else {
    Write-Output "本来就没有 $Name.tsv"
}
$global:LASTEXITCODE = 0
exit 0
