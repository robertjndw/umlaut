# Installs umlaut as a local Typst package on Windows, so it can be imported as
# `@local/umlaut:<version>`.
#
#   irm https://raw.githubusercontent.com/robertjndw/umlaut/main/install.ps1 | iex
#
# Set $env:UMLAUT_REF (branch or tag, default main) or $env:UMLAUT_NAMESPACE (default
# local) beforehand to change what gets installed. See install.sh for macOS and Linux.

$ErrorActionPreference = "Stop"

$Repo = "robertjndw/umlaut"
$Ref = if ($env:UMLAUT_REF) { $env:UMLAUT_REF } else { "main" }
$Namespace = if ($env:UMLAUT_NAMESPACE) { $env:UMLAUT_NAMESPACE } else { "local" }

# Only what the published package contains. Keep in sync with `exclude` in typst.toml.
$Files = @("typst.toml", "umlaut.wasm", "src", "README.md", "LICENSE", "THIRD-PARTY-NOTICES.md", "CHANGELOG.md")

function Get-PackageDir {
    if ($env:TYPST_PACKAGE_PATH) { return $env:TYPST_PACKAGE_PATH }
    if (Get-Command typst -ErrorAction SilentlyContinue) {
        try {
            $info = typst info --format json 2>$null | ConvertFrom-Json
            if ($info.packages.'package-path') { return $info.packages.'package-path' }
        } catch {}
    }
    return Join-Path $env:APPDATA "typst\packages"
}

$Tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("umlaut-" + [guid]::NewGuid())
New-Item -ItemType Directory -Path $Tmp | Out-Null
try {
    $Zip = Join-Path $Tmp "src.zip"
    Write-Host "Downloading $Repo@$Ref"
    Invoke-WebRequest -UseBasicParsing -Uri "https://codeload.github.com/$Repo/zip/$Ref" -OutFile $Zip
    Expand-Archive -Path $Zip -DestinationPath $Tmp
    $Source = Get-ChildItem -Path $Tmp -Directory | Select-Object -First 1

    $Toml = Get-Content (Join-Path $Source.FullName "typst.toml")
    $Name = ($Toml | Select-String -Pattern '^name\s*=\s*"(.*)"' | Select-Object -First 1).Matches[0].Groups[1].Value
    $Version = ($Toml | Select-String -Pattern '^version\s*=\s*"(.*)"' | Select-Object -First 1).Matches[0].Groups[1].Value

    $Target = Join-Path (Get-PackageDir) "$Namespace\$Name\$Version"
    # Replace rather than merge, so files removed upstream don't linger.
    if (Test-Path $Target) { Remove-Item -Recurse -Force $Target }
    New-Item -ItemType Directory -Path $Target -Force | Out-Null
    foreach ($f in $Files) {
        Copy-Item -Recurse -Path (Join-Path $Source.FullName $f) -Destination $Target
    }

    Write-Host "Installed $Name $Version to $Target"
    Write-Host "Use it with: #import `"@$Namespace/${Name}:$Version`": drawio, plantuml"
}
finally {
    Remove-Item -Recurse -Force $Tmp -ErrorAction SilentlyContinue
}
