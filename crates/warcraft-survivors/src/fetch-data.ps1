# Download the game data for the Windows player build: a World of Warcraft 1.12.1 (build 5875)
# client from archive.org, the same zip scripts/fetch-data.sh fetches, and unpack its Data folder
# into -Dir. Run by warcraft-survivors (player.rs) in its own window, which shows the progress.
#
# Resumes an interrupted download (the zip grows as `<name>.part` until it is complete); the zip
# is deleted once its Data folder is unpacked. Uses the curl.exe and tar.exe that come with
# Windows 10 and 11. Exit code 0 when Data\patch.MPQ and Data\terrain.MPQ are in place.
param(
    [Parameter(Mandatory)] [string] $Dir,
    [Parameter(Mandatory)] [string] $Url
)

$ErrorActionPreference = 'Stop'
$Host.UI.RawUI.WindowTitle = 'Warcraft Survivors: getting the game data'

function Done {
    (Test-Path (Join-Path $Dir 'Data\patch.MPQ')) -and (Test-Path (Join-Path $Dir 'Data\terrain.MPQ'))
}

function Fail([string] $Message) {
    Write-Host ''
    Write-Host $Message -ForegroundColor Red
    Write-Host 'Start Warcraft Survivors again to continue where it stopped.'
    Read-Host 'Press Enter to close this window'
    exit 1
}

if (Done) { exit 0 }
New-Item -ItemType Directory -Force $Dir | Out-Null

$name = [IO.Path]::GetFileName(([Uri] $Url).AbsolutePath)
$zip = Join-Path $Dir $name
$part = "$zip.part"

Write-Host 'Warcraft Survivors runs on the data of a World of Warcraft 1.12.1 client.'
Write-Host "It is downloaded once (about 5.5 GB) into $Dir"
Write-Host "from $Url"
Write-Host ''

if (-not (Test-Path $zip)) {
    curl.exe --location --fail --retry 5 --retry-delay 5 --continue-at - --output $part $Url
    if ($LASTEXITCODE -ne 0) { Fail "The download stopped (curl exit code $LASTEXITCODE)." }
    Move-Item -Force $part $zip
}

Write-Host ''
Write-Host 'Unpacking the game data, this takes a few minutes...'
tar.exe -xf $zip -C $Dir Data
if ($LASTEXITCODE -ne 0) { Fail "Unpacking failed (tar exit code $LASTEXITCODE)." }
if (-not (Done)) { Fail 'The download holds no Data\patch.MPQ and Data\terrain.MPQ.' }

Remove-Item -Force $zip
Write-Host 'The game data is ready.' -ForegroundColor Green
exit 0
