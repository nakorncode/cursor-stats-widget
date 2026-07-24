# Sync develop onto latest main before coding.
# Usage: .\scripts\dev-sync.ps1

$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot\..

git fetch origin
git checkout develop
git rebase origin/main
Write-Host "develop is rebased onto origin/main. Ready to work." -ForegroundColor Green
