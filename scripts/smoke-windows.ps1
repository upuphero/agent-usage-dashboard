$ErrorActionPreference = 'Stop'
if (!$env:GITHUB_ACTIONS -or !$env:RUNNER_TEMP) { throw 'Run installation smoke only on the ephemeral Actions runner.' }
$target = 'x86_64-pc-windows-msvc'
$installers = @(Get-ChildItem -LiteralPath "target/$target/release/bundle/nsis" -Filter '*.exe' -File)
if ($installers.Count -ne 1) { throw 'Exactly one NSIS installer is required.' }
$destination = Join-Path $env:RUNNER_TEMP 'Usage 安装验证'
if (Test-Path -LiteralPath $destination) { throw 'Install destination must be new.' }
# NSIS requires /D to be last; its remaining text includes spaces without quotes.
$installer = Start-Process -FilePath $installers[0].FullName -ArgumentList "/S /D=$destination" -WindowStyle Hidden -Wait -PassThru
if ($installer.ExitCode -ne 0) { throw "NSIS failed: $($installer.ExitCode)" }
node scripts/verify-bundle.mjs "--target=$target" --kind=installed "--root=$destination" "--output=artifacts/$target/bundle-manifest.json"
if ($LASTEXITCODE -ne 0) { throw 'Installed bundle verification failed.' }
$binary = Join-Path $destination 'ccusage.exe'
"CCUSAGE_TEST_BINARY=$binary" | Out-File -FilePath $env:GITHUB_ENV -Encoding utf8 -Append
