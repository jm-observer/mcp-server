param([Parameter(Mandatory=$true, Position=0)][string]$NewVersion)
if ($NewVersion -notmatch '^\d+\.\d+\.\d+$') { Write-Error "semver format required"; exit 1 }
$CargoToml = Join-Path (Split-Path -Parent (Split-Path -Parent $MyInvocation.MyCommand.Path)) "Cargo.toml"
$content = Get-Content $CargoToml -Raw
$content -match 'version\s*=\s*"(\d+\.\d+\.\d+)"' | Out-Null
$cur = $Matches[1]; Write-Host "Current: $cur -> New: $NewVersion"
if ($cur -eq $NewVersion) { Write-Error "same version"; exit 1 }
$content = $content -replace "version = `"$cur`"", "version = `"$NewVersion`""
Set-Content -Path $CargoToml -Value $content -NoNewline
cargo check --workspace 2>&1 | Select-Object -Last 1
git add Cargo.toml && git commit -m "chore: bump version to v$NewVersion" && git tag "v$NewVersion"
Write-Host "Done! Run: git push && git push --tags"
