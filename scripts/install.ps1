param(
    [string]$Version = $env:CRAB_VERSION,
    [string]$InstallDir = $(if ($env:CRAB_INSTALL_DIR) { $env:CRAB_INSTALL_DIR } else { Join-Path $HOME ".crab\bin" })
)

$ErrorActionPreference = "Stop"
$Repo = "matingai/crab"

if (-not $Version) {
    $Latest = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases?per_page=1"
    $Version = @($Latest)[0].tag_name
}

if (-not $Version) {
    throw "failed to resolve latest Crab release version"
}

$Arch = $env:PROCESSOR_ARCHITECTURE
if ($Arch -ne "AMD64") {
    throw "unsupported Windows architecture: $Arch. The current release ships Windows x64 builds."
}

$Target = "x86_64-pc-windows-msvc"
$Package = "crab-$Version-$Target"
$Archive = "$Package.zip"
$BaseUrl = "https://github.com/$Repo/releases/download/$Version"
$TempDir = Join-Path ([System.IO.Path]::GetTempPath()) ([System.Guid]::NewGuid().ToString())

New-Item -ItemType Directory -Path $TempDir | Out-Null
try {
    Push-Location $TempDir

    Write-Host "Downloading $Archive"
    Invoke-WebRequest -Uri "$BaseUrl/$Archive" -OutFile $Archive
    Invoke-WebRequest -Uri "$BaseUrl/$Archive.sha256" -OutFile "$Archive.sha256"

    $Expected = (Get-Content "$Archive.sha256" -Raw).Trim().Split(" ", [System.StringSplitOptions]::RemoveEmptyEntries)[0].ToLowerInvariant()
    $Actual = (Get-FileHash $Archive -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($Expected -ne $Actual) {
        throw "checksum mismatch for $Archive"
    }

    Expand-Archive -Path $Archive -DestinationPath . -Force
    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
    Copy-Item -Path (Join-Path $Package "crab.exe") -Destination (Join-Path $InstallDir "crab.exe") -Force

    Write-Host "Installed crab to $(Join-Path $InstallDir 'crab.exe')"
    & (Join-Path $InstallDir "crab.exe") --version
    Write-Host "Add $InstallDir to PATH to run crab from any directory."
}
finally {
    Pop-Location
    Remove-Item -Path $TempDir -Recurse -Force
}
