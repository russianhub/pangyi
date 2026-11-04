# 把安装包和 SHA256 上传到标签对应的草稿 Release。
# 说明正文就是 docs/releases/<标签>.md，不另写一套短说明。
# 标签已有 Release（含草稿）时失败，避免覆盖已有附件。
param([switch]$CheckOnly)

$ErrorActionPreference = "Stop"

$Tag = $env:RELEASE_TAG
if (-not $Tag) { throw "RELEASE_TAG is empty" }
if ($Tag -notmatch "^v\d") { throw "refusing tag that does not look like v<version>: $Tag" }
if ($env:GITHUB_REF_TYPE -and $env:GITHUB_REF_TYPE -ne "tag") {
    throw "release workflow only runs for tags (GITHUB_REF_TYPE=$env:GITHUB_REF_TYPE)"
}
if (-not $env:GH_TOKEN) { throw "GH_TOKEN is empty" }

$Repo = if ($env:GITHUB_WORKSPACE) { $env:GITHUB_WORKSPACE } else { Get-Location }
$notesFile = Join-Path $Repo "docs\releases\$Tag.md"
if (-not (Test-Path -LiteralPath $notesFile)) {
    throw "release notes missing: docs/releases/$Tag.md"
}

function Assert-ReleaseAbsent {
    param([string]$Name)
    $prev = $PSNativeCommandUseErrorActionPreference
    $PSNativeCommandUseErrorActionPreference = $false
    $out = & gh release view $Name --repo $env:GITHUB_REPOSITORY --json tagName,isDraft,url 2>&1 | Out-String
    $code = $LASTEXITCODE
    $PSNativeCommandUseErrorActionPreference = $prev
    if ($code -eq 0) {
        throw "Release $Name already exists. Refusing to upload or replace assets.`n$out"
    }
    if ($out -notmatch "(?i)not found") {
        throw "Could not confirm that $Name has no release. Not uploading.`n$out"
    }
    Write-Host "no existing release for $Name"
}

Assert-ReleaseAbsent $Tag
if ($CheckOnly) { return }

Set-Location $Repo
$built = Join-Path $Repo "ime\target\release\setup.exe"
if (-not (Test-Path $built)) { throw "installer missing: $built" }

$dist = Join-Path $Repo "dist"
New-Item -ItemType Directory -Force -Path $dist | Out-Null
$exeName = "pangyi-$Tag.exe"
$sumName = "$exeName.sha256"
$exe = Join-Path $dist $exeName
$sum = Join-Path $dist $sumName
Copy-Item $built $exe -Force
$hash = (Get-FileHash -Algorithm SHA256 $exe).Hash.ToLower()
$utf8 = New-Object System.Text.UTF8Encoding $false
[System.IO.File]::WriteAllText($sum, "$hash  $exeName`n", $utf8)

# 创建前再查一次，缩短「检查之后、创建之前」被人抢先发布的窗口。
# --draft：保持草稿，不公开发布。正文用 docs/releases/<标签>.md 原文。
Write-Host "release body: docs/releases/$Tag.md"
Assert-ReleaseAbsent $Tag
& gh release create $Tag `
    --repo $env:GITHUB_REPOSITORY `
    --draft `
    --verify-tag `
    --title "旁译 $Tag" `
    --notes-file $notesFile `
    $exe `
    $sum

Write-Host "draft release created for $Tag"
Write-Host "$exeName $hash"
