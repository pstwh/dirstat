$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

$repo = 'pstwh/dirstat'
$installDir = if ($env:DIRSTAT_INSTALL_DIR) { $env:DIRSTAT_INSTALL_DIR } else { Join-Path $env:LOCALAPPDATA 'Programs\dirstat' }

if (-not [Environment]::Is64BitOperatingSystem) {
    throw 'dirstat needs 64-bit Windows'
}

if ($env:DIRSTAT_VERSION) {
    $tag = 'v' + $env:DIRSTAT_VERSION.TrimStart('v')
} else {
    $release = Invoke-RestMethod -Uri "https://api.github.com/repos/$repo/releases/latest" -Headers @{ 'User-Agent' = 'dirstat-installer' }
    $tag = $release.tag_name
}

$name = "dirstat-$tag-x86_64-windows"
$base = "https://github.com/$repo/releases/download/$tag"
$work = Join-Path ([IO.Path]::GetTempPath()) ([Guid]::NewGuid().ToString())
New-Item -ItemType Directory -Path $work | Out-Null

try {
    Write-Host "Downloading dirstat $tag for x86_64-windows"
    $zip = Join-Path $work "$name.zip"
    $sums = Join-Path $work 'SHA256SUMS.txt'
    Invoke-WebRequest -Uri "$base/$name.zip" -OutFile $zip -UseBasicParsing
    Invoke-WebRequest -Uri "$base/SHA256SUMS.txt" -OutFile $sums -UseBasicParsing

    $line = Get-Content $sums | Where-Object { $_ -match "\s$([regex]::Escape("$name.zip"))$" } | Select-Object -First 1
    if (-not $line) {
        throw "$name.zip is missing from SHA256SUMS.txt"
    }
    $expected = ($line -split '\s+')[0].ToLower()
    $actual = (Get-FileHash -Algorithm SHA256 $zip).Hash.ToLower()
    if ($expected -ne $actual) {
        throw "checksum mismatch for $name.zip"
    }

    Get-Process dirstat -ErrorAction SilentlyContinue | Where-Object { $_.Path -like "$installDir*" } | Stop-Process -Force
    Expand-Archive -Path $zip -DestinationPath $work -Force
    New-Item -ItemType Directory -Path $installDir -Force | Out-Null
    Copy-Item (Join-Path $work "$name\dirstat.exe") (Join-Path $installDir 'dirstat.exe') -Force
} finally {
    Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue
}

$exe = Join-Path $installDir 'dirstat.exe'
$menu = Join-Path ([Environment]::GetFolderPath('Programs')) 'dirstat.lnk'
$shell = New-Object -ComObject WScript.Shell
$shortcut = $shell.CreateShortcut($menu)
$shortcut.TargetPath = $exe
$shortcut.WorkingDirectory = $installDir
$shortcut.Save()

$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
$entries = if ($userPath) { $userPath -split ';' } else { @() }
if ($entries -notcontains $installDir) {
    [Environment]::SetEnvironmentVariable('Path', (($entries + $installDir) | Where-Object { $_ }) -join ';', 'User')
    $env:Path = "$env:Path;$installDir"
    Write-Host "Added $installDir to your PATH, open a new terminal to use it"
}

Write-Host "Installed $exe and a Start menu entry"
