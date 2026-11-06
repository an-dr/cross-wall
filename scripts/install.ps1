# Installs the latest cross-wall release for this user. Needs no administrator rights.
$ErrorActionPreference = 'Stop'
$arch = if ([Runtime.InteropServices.RuntimeInformation]::OSArchitecture -eq 'Arm64') { 'arm64' } else { 'x64' }
$base = 'https://github.com/an-dr/cross-wall/releases/latest/download'
$manifest = Invoke-RestMethod "$base/cross-wall-windows-$arch.json"
$work = Join-Path ([IO.Path]::GetTempPath()) "cross-wall-$([guid]::NewGuid())"
New-Item -ItemType Directory $work | Out-Null
try {
    $zip = Join-Path $work 'cross-wall.zip'
    Invoke-WebRequest $manifest.url -OutFile $zip
    if ((Get-FileHash $zip -Algorithm SHA256).Hash -ne $manifest.sha256) { throw 'The download does not match its checksum.' }
    Expand-Archive $zip (Join-Path $work 'app')
    $app = Join-Path $work 'app/cross-wall-app.exe'
    $process = Start-Process $app '--install' -Wait -PassThru
    if ($process.ExitCode -ne 0) { throw 'Installation failed.' }
    Write-Host 'cross-wall is installed. Start it from the Start Menu.'
} finally {
    Remove-Item $work -Recurse -Force
}
