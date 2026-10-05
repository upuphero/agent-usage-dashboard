$ErrorActionPreference = 'Stop'
if (!$env:GITHUB_ACTIONS -or !$env:RUNNER_TEMP) { throw 'Build portable package only on the ephemeral Windows Actions runner.' }
$target = 'x86_64-pc-windows-msvc'
$installed = Join-Path $env:RUNNER_TEMP 'Usage 安装验证'
$folderName = 'Agent Usage Dashboard'
$staging = Join-Path $env:RUNNER_TEMP 'Usage portable staging'
$folder = Join-Path $staging $folderName
$unpacked = Join-Path $env:RUNNER_TEMP 'Usage 免安装验证'
foreach ($directory in @($staging, $unpacked)) {
    if (Test-Path -LiteralPath $directory) { throw 'Portable staging and verification directories must be new.' }
}
New-Item -ItemType Directory -Path $folder | Out-Null
foreach ($name in @('usage-desktop.exe','ccusage.exe')) {
    Copy-Item -LiteralPath (Join-Path $installed $name) -Destination (Join-Path $folder $name)
}
Copy-Item -LiteralPath 'THIRD_PARTY_NOTICES.md' -Destination $folder
Copy-Item -LiteralPath 'LICENSE' -Destination $folder
@'
解压后双击 usage-desktop.exe。请保留整个文件夹，ccusage.exe 不能移走。
首次使用进入数据来源，点击 Codex / Antigravity 的“启用并扫描”。
用量数据保存在当前用户的应用数据目录，与安装版共享历史；不是把数据库写到 U 盘的模式。
需要系统已有 Microsoft Edge WebView2 Runtime；Windows 未签名。
'@ | Out-File -LiteralPath (Join-Path $folder '开始使用.txt') -Encoding utf8
$version = (Get-Content -LiteralPath 'package.json' -Raw | ConvertFrom-Json).version
$zip = Join-Path $PWD "artifacts/$target/Agent Usage Dashboard_${version}_x64-portable.zip"
Compress-Archive -LiteralPath $folder -DestinationPath $zip -CompressionLevel Optimal
Expand-Archive -LiteralPath $zip -DestinationPath $unpacked
$root = Join-Path $unpacked $folderName
node scripts/verify-bundle.mjs "--target=$target" --kind=portable "--root=$root" "--output=artifacts/$target/portable-manifest.json"
if ($LASTEXITCODE -ne 0) { throw 'Extracted portable package verification failed.' }
"CCUSAGE_TEST_BINARY=$(Join-Path $root 'ccusage.exe')" | Out-File -FilePath $env:GITHUB_ENV -Encoding utf8 -Append
