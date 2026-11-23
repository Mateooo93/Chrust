# Aurora installer. The release build compiles this with ps2exe into
# aurora-setup.exe and embeds the browser and its icon in it.
#
# Installs for the current user only (no administrator needed): the browser,
# its icon, a Start menu entry and an entry in Apps and Features. Running it
# again replaces an existing install in place.

param([switch]$quiet)

Add-Type -AssemblyName System.Windows.Forms
$ErrorActionPreference = 'Stop'
$appDir = Join-Path $env:LOCALAPPDATA 'Programs\Aurora'
$target = Join-Path $appDir 'aurora.exe'
$startDir = Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs'
$startLink = Join-Path $startDir 'Aurora.lnk'
$uninstallKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Aurora'
$version = if ($env:AURORA_VERSION) { $env:AURORA_VERSION } else { '0.0.0' }

# The release build appends a zip with the browser and its icon to this exe.
# A zip reader searches for its directory from the end of the file, so the
# first run can open it, pull the two files out beside the exe, and carry on.
function Expand-Payload {
    $self = $MyInvocation.MyCommand.Path
    if (-not $self) { $self = [System.Diagnostics.Process]::GetCurrentProcess().MainModule.FileName }
    $folder = Split-Path -Parent $self
    $exe = Join-Path $folder 'aurora.exe'
    $icon = Join-Path $folder 'aurora.ico'
    if (-not (Test-Path $exe)) {
        $zip = "$self.zip"
        try {
            Copy-Item -Force $self $zip
            Add-Type -AssemblyName System.IO.Compression.FileSystem
            $archive = [System.IO.Compression.ZipFile]::OpenRead($zip)
            foreach ($entry in $archive.Entries) {
                $out = Join-Path $folder ([System.IO.Path]::GetFileName($entry.FullName))
                if ($entry.FullName -and $out) {
                    [System.IO.Compression.ZipFileExtensions]::ExtractToFile($entry, $out, $true)
                }
            }
            $archive.Dispose()
        } catch {
            # Not packaged with a payload (or a partial file); fall through to
            # the files sitting beside the exe.
        } finally {
            Remove-Item -Force $zip -ErrorAction SilentlyContinue
        }
    }
    return @{ Exe = $exe; Icon = $icon }
}

function Install-Aurora($files) {
    if (-not (Test-Path $files.Exe)) { throw 'The installer is missing the browser files.' }
    New-Item -ItemType Directory -Force -Path $appDir | Out-Null
    New-Item -ItemType Directory -Force -Path $startDir | Out-Null
    Copy-Item -Force $files.Exe $target
    $icon = Join-Path $appDir 'aurora.ico'
    if (Test-Path $files.Icon) { Copy-Item -Force $files.Icon $icon }

    $shell = New-Object -ComObject WScript.Shell
    $link = $shell.CreateShortcut($startLink)
    $link.TargetPath = $target
    $link.WorkingDirectory = $appDir
    if (Test-Path $icon) { $link.IconLocation = $icon }
    $link.Description = 'Aurora browser'
    $link.Save()

    New-Item -Path $uninstallKey -Force | Out-Null
    Set-ItemProperty $uninstallKey -Name DisplayName -Value 'Aurora'
    Set-ItemProperty $uninstallKey -Name DisplayVersion -Value $version
    Set-ItemProperty $uninstallKey -Name Publisher -Value 'Aurora'
    Set-ItemProperty $uninstallKey -Name InstallLocation -Value $appDir
    Set-ItemProperty $uninstallKey -Name DisplayIcon -Value $target
    Set-ItemProperty $uninstallKey -Name NoModify -Type DWord -Value 1
    Set-ItemProperty $uninstallKey -Name NoRepair -Type DWord -Value 1
    $uninstall = Join-Path $appDir 'uninstall.cmd'
    $lines = @(
        '@echo off',
        'rem Removes Aurora. Bookmarks, history and settings are kept.',
        'reg delete "HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall\Aurora" /f >nul 2>&1',
        "del `"$startLink`" >nul 2>&1",
        'del "%~dp0aurora.exe" >nul 2>&1',
        'del "%~dp0aurora.ico" >nul 2>&1',
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
            "Install Aurora $version for your account?`n`nBookmarks, history and settings are kept if you already use Aurora.",
            'Aurora setup',
            [System.Windows.Forms.MessageBoxButtons]::OKCancel,
            [System.Windows.Forms.MessageBoxIcon]::Information)
        if ($answer -ne [System.Windows.Forms.DialogResult]::OK) { exit 0 }
    }
    $exe = Install-Aurora (Expand-Payload)
    if (-not $quiet) {
        $start = [System.Windows.Forms.MessageBox]::Show(
            'Aurora is installed. Start it now?', 'Aurora setup',
            [System.Windows.Forms.MessageBoxButtons]::YesNo,
            [System.Windows.Forms.MessageBoxIcon]::Information)
        if ($start -eq [System.Windows.Forms.DialogResult]::Yes) { Start-Process $exe }
    }
} catch {
    if (-not $quiet) {
        [System.Windows.Forms.MessageBox]::Show("Aurora could not be installed:`n$_", 'Aurora setup', 'OK', 'Error') | Out-Null
    }
    exit 1
}
