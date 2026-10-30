# CCS/1 fetch WinDivert 2.2.2 x64 (LGPL-3.0) into src-tauri/windivert; refuses changed files or unsigned driver.
# GitHub publishes no digest and upstream signs only the .sys: hashes pinned from the official release, 2026-10-30.
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
$pinned = @{
    'WinDivert.dll'   = 'C1E060EE19444A259B2162F8AF0F3FE8C4428A1C6F694DCE20DE194AC8D7D9A2'
    'WinDivert64.sys' = '8DA085332782708D8767BCACE5327A6EC7283C17CFB85E40B03CD2323A90DDC2'
}
foreach ($file in $pinned.Keys) {
    if ((Get-FileHash (Join-Path $dir $file)).Hash -ne $pinned[$file]) { throw "$file hash mismatch" }
}
$sig = Get-AuthenticodeSignature (Join-Path $dir 'WinDivert64.sys')
if ($sig.Status -ne 'Valid') { throw "WinDivert64.sys signature: $($sig.Status)" }
