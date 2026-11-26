# Official standalone-shell build entry (T29 / T32).
# Does not modify the main-program packager, Cargo workspace, or ui/node_modules.
# Usage (from apps/bgxiong-prompt-optimize or any cwd):
#   powershell -NoProfile -ExecutionPolicy Bypass -File scripts/build-official.ps1
# Optional:
#   -SkipNative     frontend + resource checks only
#   -SkipNpmCi      reuse this tree's already-installed shell node_modules

param(
  [switch]$SkipNative,
  [switch]$SkipNpmCi,
  [switch]$Development,
  [string]$LogDir,
  [string]$RuntimeDirectory
)

$ErrorActionPreference = "Continue"
$appRoot = Split-Path -Parent $PSScriptRoot
$repoRoot = (Resolve-Path (Join-Path $appRoot "../..")).Path
$tauriDir = Join-Path $appRoot "src-tauri"
if (-not $LogDir) {
  $LogDir = Join-Path $appRoot "build-official-logs"
}
New-Item -ItemType Directory -Force -Path $LogDir | Out-Null
$summary = Join-Path $LogDir "summary.log"

function Log([string]$msg) {
  $line = "[{0}] {1}" -f (Get-Date -Format "yyyy-MM-dd HH:mm:ss"), $msg
  Write-Host $line
  Add-Content -LiteralPath $summary -Value $line -Encoding UTF8
}

function Invoke-Checked([string]$name, [string]$workDir, [string]$command) {
  $out = Join-Path $LogDir ($name + ".log")
  Log ("STEP " + $name + " cwd=" + $workDir + " cmd=" + $command)
  Push-Location -LiteralPath $workDir
  try {
    cmd /c "$command > `"$out`" 2>&1"
    $code = $LASTEXITCODE
  } finally {
    Pop-Location
  }
  Log ("STEP_END " + $name + " exit=" + $code + " log=" + $out)
  if ($code -ne 0) { throw "$name failed exit=$code; see $out" }
}

if (Test-Path -LiteralPath $summary) { Remove-Item -LiteralPath $summary -Force }
Log "OFFICIAL_SHELL_BUILD_START"
Log ("app=" + $appRoot)
Log ("repo=" + $repoRoot)
Set-Location -LiteralPath $appRoot
Log ("HEAD=" + (git -C $repoRoot rev-parse HEAD))
Log ("node=" + (node -v) + " npm=" + (npm -v))

# Never npm ci through a ui/node_modules junction.
$uiNm = Join-Path $repoRoot "ui\node_modules"
if (Test-Path -LiteralPath $uiNm) {
  $item = Get-Item -LiteralPath $uiNm -Force
  Log ("ui/node_modules Attributes=" + $item.Attributes + " LinkType=" + $item.LinkType)
  if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) {
    Log "NOTE ui/node_modules is a reparse point; this official script does not use or npm ci through it"
  }
}

if (-not $SkipNpmCi) {
  if (-not (Test-Path -LiteralPath (Join-Path $appRoot "package-lock.json"))) {
    throw "missing official apps/bgxiong-prompt-optimize/package-lock.json"
  }
  Invoke-Checked "npm-ci" $appRoot "npm ci --loglevel info --no-fund --no-audit"
}

Invoke-Checked "check-layout" $appRoot "node scripts/check-layout-contract.mjs"
Invoke-Checked "check-boundary" $appRoot "node scripts/check-import-boundary.mjs"
Invoke-Checked "check-resources" $appRoot "node scripts/check-resource-bundle.mjs"
Invoke-Checked "check-shared-ui" $appRoot "node scripts/check-shared-ui-isolation.mjs"
Invoke-Checked "host-input-lock" $repoRoot "python scripts/sync-host-inputs.py --check"
Invoke-Checked "frontend-build" $appRoot "npm run build"

if ($SkipNative) {
  Log "OFFICIAL_SHELL_BUILD_END frontend-only ok (native Tauri skipped)"
  exit 0
}

Log ("cargo=" + (cargo --version))
Log ("tauri-cli=" + ((cargo tauri --version) | Out-String).Trim())
$savedTarget = $env:CARGO_TARGET_DIR
$target = if ($savedTarget) { [IO.Path]::GetFullPath($savedTarget) } else { Join-Path $repoRoot "target" }
$env:CARGO_TARGET_DIR = $target
try {
  $profile = if ($Development) { "debug" } else { "release" }
  $cargoProfile = if ($Development) { "" } else { "--release" }
  Invoke-Checked "cli-build" $repoRoot "cargo build --locked -p prompt-optimize-runtime --bin bgxiong-prompt-optimize-cli $cargoProfile"
  $cli = Join-Path $target "$profile/bgxiong-prompt-optimize-cli.exe"
  $component = Join-Path $tauriDir "component-staging"
  New-Item -ItemType Directory -Path $component -Force | Out-Null
  Copy-Item -LiteralPath $cli -Destination $component
  $overlay = Join-Path $component "tauri.component.json"
  $configuration = @{bundle = @{resources = @{
    "component-staging/bgxiong-prompt-optimize-cli.exe" = "bgxiong-prompt-optimize-cli.exe"
  }}} | ConvertTo-Json -Depth 5
  [IO.File]::WriteAllText($overlay, $configuration, [Text.UTF8Encoding]::new($false))
  $tauriProfile = if ($Development) { "--debug --no-bundle" } else { "" }
  Invoke-Checked "tauri-build" $tauriDir "cargo tauri build $tauriProfile --config component-staging/tauri.component.json -- --locked"
  if ($RuntimeDirectory) {
    $gui = Join-Path $target "$profile/bgxiong-prompt-optimize.exe"
    & python (Join-Path $repoRoot "scripts/stage-runtime.py") --gui $gui --cli $cli --output $RuntimeDirectory --profile $profile
    if ($LASTEXITCODE -ne 0) { throw "Could not stage the optimizer runtime." }
  }
} finally {
  $env:CARGO_TARGET_DIR = $savedTarget
}
Log "OFFICIAL_SHELL_BUILD_END ok"
