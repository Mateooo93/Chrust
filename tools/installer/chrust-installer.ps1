# Chrust installer. The release build compiles this with ps2exe into
# chrust-setup.exe and appends a zip holding `chrust.exe` and `chrust.ico`.
#
# A zip reader finds its directory by searching backwards from the end of the
# file, so the appended zip is readable from inside the setup executable by
# copying the executable and asking the reader for the archive at an offset:
# the bytes before that offset are the executable itself, which is skipped
# with a single entry whose data is the whole exe (a "stored" range that is
# never extracted).
#
# Installs for the current user only (no administrator needed): the browser,
# its icon, a Start menu entry and an entry in Apps and Features. Running it
# again replaces an existing install in place.

param([switch]$quiet)

Add-Type -AssemblyName System.Windows.Forms
$ErrorActionPreference = 'Stop'
$appDir = Join-Path $env:LOCALAPPDATA 'Programs\Chrust'
$target = Join-Path $appDir 'chrust.exe'
$iconTarget = Join-Path $appDir 'chrust.ico'
$startDir = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs'
$startLink = Join-Path $startDir 'Chrust.lnk'
$uninstallKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Chrust'
# ps2exe embeds this script, so the release build writes the version in as
# a constant before packaging.
$version = '0.0.0'

function Get-SelfPath {
    $process = [System.Diagnostics.Process]::GetCurrentProcess()
    return $process.MainModule.FileName
}

# Pulls chrust.exe and chrust.ico out of the zip appended to this executable.
#
# The setup exe is this script (compiled), with the zip appended. The zip's
# end record says how large its directory is and how far it begins; both are
# relative to the start of the zip, which is therefore the end record's
# position minus the directory size minus that distance.
function Expand-Payload([string]$folder) {
    $self = Get-SelfPath
    $bytes = [System.IO.File]::ReadAllBytes($self)
    $exe = Join-Path $folder 'chrust.exe'
    $icon = Join-Path $folder 'chrust.ico'
    if ((Test-Path $exe) -and ((Get-Item $exe).Length -gt 100KB)) {
        return @{ Exe = $exe; Icon = $icon }
    }

    # End-of-directory record: PK\x05\x06, searched from the end. Its comment
    # field must account for the rest of the file.
    $end = -1
    for ($i = $bytes.Length - 22; $i -ge 0; $i--) {
        if ($bytes[$i] -eq 0x50 -and $bytes[$i + 1] -eq 0x4B -and $bytes[$i + 2] -eq 0x05 -and $bytes[$i + 3] -eq 0x06) {
            $comment = [BitConverter]::ToUInt16($bytes, $i + 20)
            if ($i + 22 + $comment -eq $bytes.Length) { $end = $i; break }
        }
    }
    if ($end -lt 0) { throw 'The installer is missing the browser files.' }
    $count = [BitConverter]::ToUInt16($bytes, $end + 10)
    $dirSize = (Get-UInt32 $bytes ($end + 12))
    $dirOffset = (Get-UInt32 $bytes ($end + 16))
    if ($count -eq 0 -or ($dirSize + $dirOffset) -gt $end) { throw 'The installer is missing the browser files.' }
    $zipStart = $end - $dirSize - $dirOffset

    # Directory records: 46 bytes each plus their name, extra and comment.
    $at = [int64]$end - $dirSize
    for ($n = 0; $n -lt $count; $n++) {
        if (-not ($bytes[$at] -eq 0x50 -and $bytes[$at + 1] -eq 0x4B -and $bytes[$at + 2] -eq 0x01 -and $bytes[$at + 3] -eq 0x02)) { throw 'The installer payload is damaged.' }
        $method = [BitConverter]::ToUInt16($bytes, $at + 10)
        $packed = (Get-UInt32 $bytes ($at + 20))
        $nameLen = [BitConverter]::ToUInt16($bytes, $at + 28)
        $extraLen = [BitConverter]::ToUInt16($bytes, $at + 30)
        $commentLen = [BitConverter]::ToUInt16($bytes, $at + 32)
        $local = (Get-UInt32 $bytes ($at + 42))
        $name = [System.IO.Path]::GetFileName([System.Text.Encoding]::UTF8.GetString($bytes, $at + 46, $nameLen))
        if ($name -eq 'chrust.exe' -or $name -eq 'chrust.ico') {
            # The local header repeats its name and extra lengths.
            $head = $zipStart + $local
            $dataAt = $head + 30 + [BitConverter]::ToUInt16($bytes, $head + 26) + [BitConverter]::ToUInt16($bytes, $head + 28)
            $raw = $bytes[$dataAt..($dataAt + $packed - 1)]
            $data = if ($method -eq 0) { $raw } else { Expand-Bytes $raw }
            [System.IO.File]::WriteAllBytes((Join-Path $folder $name), [byte[]]$data)
        }
        $at += 46 + $nameLen + $extraLen + $commentLen
    }
    if (-not (Test-Path $exe)) { throw 'The installer is missing the browser files.' }
    return @{ Exe = $exe; Icon = $icon }
}

function Get-UInt32([byte[]]$bytes, [int]$at) {
    return ([int64][BitConverter]::ToUInt32($bytes, $at))
}

# Raw deflate, as every zip writer uses for entries it compresses.
function Expand-Bytes([byte[]]$raw) {
    $input = [System.IO.MemoryStream]::new($raw)
    $inflate = [System.IO.Compression.DeflateStream]::new($input, [System.IO.Compression.CompressionMode]::Decompress)
    $output = [System.IO.MemoryStream]::new()
    try { $inflate.CopyTo($output) } finally { $inflate.Dispose() }
    return $output.ToArray()
}

function Install-Chrust($files) {
    if (-not (Test-Path $files.Exe)) { throw 'The installer is missing the browser files.' }
    New-Item -ItemType Directory -Force -Path $appDir | Out-Null
    New-Item -ItemType Directory -Force -Path $startDir | Out-Null
    Copy-Item -Force $files.Exe $target
    if (Test-Path $files.Icon) { Copy-Item -Force $files.Icon $iconTarget }

    $shell = New-Object -ComObject WScript.Shell
    $link = $shell.CreateShortcut($startLink)
    $link.TargetPath = $target
    $link.WorkingDirectory = $appDir
    if (Test-Path $iconTarget) { $link.IconLocation = $iconTarget }
    $link.Description = 'Chrust browser'
    $link.Save()

    New-Item -Path $uninstallKey -Force | Out-Null
    Set-ItemProperty $uninstallKey -Name DisplayName -Value 'Chrust'
    Set-ItemProperty $uninstallKey -Name DisplayVersion -Value $version
    Set-ItemProperty $uninstallKey -Name Publisher -Value 'Chrust'
    Set-ItemProperty $uninstallKey -Name InstallLocation -Value $appDir
    Set-ItemProperty $uninstallKey -Name DisplayIcon -Value $target
    Set-ItemProperty $uninstallKey -Name NoModify -Type DWord -Value 1
    Set-ItemProperty $uninstallKey -Name NoRepair -Type DWord -Value 1
    $uninstall = Join-Path $appDir 'uninstall.cmd'
    $lines = @(
        '@echo off',
        'rem Removes Chrust. Bookmarks, history and settings are kept.',
        'reg delete "HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\Chrust" /f >nul 2>&1',
        "del `"$startLink`" >nul 2>&1",
        'del "%~dp0chrust.exe" >nul 2>&1',
        'del "%~dp0chrust.ico" >nul 2>&1',
        'del "%~dp0uninstall.cmd" >nul 2>&1',
        'rd "%~dp0" >nul 2>&1'
    )
    Set-Content -Encoding ASCII -Path $uninstall -Value $lines
    Set-ItemProperty $uninstallKey -Name UninstallString -Value "`"$uninstall`""
    return $target
}

try {
    if (-not $quiet) {
        $answer = [System.Windows.Forms.MessageBox]::Show(
            "Install Chrust $version for your account?`n`nBookmarks, history and settings are kept if you already use Chrust.",
            'Chrust setup',
            [System.Windows.Forms.MessageBoxButtons]::OKCancel,
            [System.Windows.Forms.MessageBoxIcon]::Information)
        if ($answer -ne [System.Windows.Forms.DialogResult]::OK) { exit 0 }
    }
    $staging = Join-Path $env:TEMP ('chrust-setup-' + [guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Force -Path $staging | Out-Null
    try {
        $files = Expand-Payload $staging
        $exe = Install-Chrust $files
    } finally {
        Remove-Item -Recurse -Force $staging -ErrorAction SilentlyContinue
    }
    if (-not $quiet) {
        $start = [System.Windows.Forms.MessageBox]::Show(
            'Chrust is installed. Start it now?', 'Chrust setup',
            [System.Windows.Forms.MessageBoxButtons]::YesNo,
            [System.Windows.Forms.MessageBoxIcon]::Information)
        if ($start -eq [System.Windows.Forms.DialogResult]::Yes) { Start-Process $exe }
    }
} catch {
    if (-not $quiet) {
        [System.Windows.Forms.MessageBox]::Show("Chrust could not be installed:`n$_", 'Chrust setup', 'OK', 'Error') | Out-Null
    }
    exit 1
}
