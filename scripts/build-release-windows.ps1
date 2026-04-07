$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
$tauriCli = Join-Path $repoRoot "frontend\node_modules\.bin\tauri.cmd"

if (-not (Test-Path $tauriCli)) {
    Write-Error "Missing local Tauri CLI at $tauriCli. Run 'cd frontend; npm install' first."
}

Push-Location $repoRoot
try {
    & $tauriCli build --config src-tauri/tauri.conf.json
}
finally {
    Pop-Location
}
