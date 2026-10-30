# CCS/1 fetch WinDivert 2.2.2 x64 (LGPL-3.0) into src-tauri/windivert; refuses unsigned driver/dll.
# ponytail: GitHub publishes no digest for this zip; Authenticode on .sys/.dll is the check.
$ErrorActionPreference = 'Stop'
$dir = Join-Path $PSScriptRoot 'windivert'
$zip = Join-Path $dir 'WinDivert-2.2.2-A.zip'
New-Item -ItemType Directory -Force $dir | Out-Null
Invoke-WebRequest https://github.com/basil00/WinDivert/releases/download/v2.2.2/WinDivert-2.2.2-A.zip -OutFile $zip
Add-Type -AssemblyName System.IO.Compression.FileSystem
$archive = [IO.Compression.ZipFile]::OpenRead($zip)
try {
    foreach ($name in 'x64/WinDivert.dll', 'x64/WinDivert64.sys', 'LICENSE') {
        $entry = $archive.GetEntry("WinDivert-2.2.2-A/$name")
        [IO.Compression.ZipFileExtensions]::ExtractToFile($entry, (Join-Path $dir (Split-Path $name -Leaf)), $true)
    }
} finally { $archive.Dispose(); Remove-Item $zip }
foreach ($file in 'WinDivert.dll', 'WinDivert64.sys') {
    $sig = Get-AuthenticodeSignature (Join-Path $dir $file)
    if ($sig.Status -ne 'Valid') { throw "$file signature: $($sig.Status)" }
    "$file signed by $($sig.SignerCertificate.Subject)"
}
