$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
$targetDir = Join-Path $repoRoot "target\release"
$bundleDir = Join-Path $targetDir "bundle"

$candidate = Get-ChildItem (Join-Path $bundleDir "msi") -Filter *.msi -Recurse -ErrorAction SilentlyContinue |
    Select-Object -First 1

if (-not $candidate) {
    $candidate = Get-ChildItem (Join-Path $bundleDir "nsis") -Filter *.exe -Recurse -ErrorAction SilentlyContinue |
        Select-Object -First 1
}

if (-not $candidate) {
    $exePath = Join-Path $targetDir "flick-desktop.exe"
    if (Test-Path $exePath) {
        $candidate = Get-Item $exePath
    }
}

if (-not $candidate) {
    Write-Error "No Windows installer or executable found under $targetDir. Run the release build first."
}

Start-Process $candidate.FullName
