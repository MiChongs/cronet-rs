[CmdletBinding()]
param(
    [ValidateSet("amd64", "arm64")]
    [string] $Architecture,
    [string] $Destination = "target/cronet-sdk"
)

$ErrorActionPreference = "Stop"
$release = "v148.0.7778.96-1"

if (-not $Architecture) {
    $Architecture = switch ($env:PROCESSOR_ARCHITECTURE) {
        "AMD64" { "amd64" }
        "ARM64" { "arm64" }
        default { throw "Unsupported Windows architecture: $env:PROCESSOR_ARCHITECTURE" }
    }
}

$root = Split-Path -Parent $PSScriptRoot
$checksums = Join-Path $root "native/SHA256SUMS"
$asset = "libcronet-windows-$Architecture.dll"
$checksumMatch = Select-String -LiteralPath $checksums -Pattern "^[0-9a-f]{64}\s+$([regex]::Escape($asset))$"
if (-not $checksumMatch) {
    throw "No checksum is pinned for $asset"
}
$expected = $checksumMatch.Line.Split()[0]

$destinationPath = if ([IO.Path]::IsPathRooted($Destination)) {
    [IO.Path]::GetFullPath($Destination)
} else {
    [IO.Path]::GetFullPath((Join-Path $root $Destination))
}
$bin = Join-Path $destinationPath "bin"
$lib = Join-Path $destinationPath "lib"
New-Item -ItemType Directory -Force -Path $bin, $lib | Out-Null
$download = Join-Path $bin $asset
$url = "https://github.com/SagerNet/cronet-go/releases/download/$release/$asset"
Invoke-WebRequest -Uri $url -OutFile $download
$actual = (Get-FileHash -Algorithm SHA256 -LiteralPath $download).Hash.ToLowerInvariant()
if ($actual -ne $expected) {
    Remove-Item -LiteralPath $download
    throw "SHA-256 mismatch for $asset`: expected $expected, received $actual"
}

$readObject = Get-Command llvm-readobj -ErrorAction SilentlyContinue
if ($readObject) {
    $exported = & $readObject.Source --coff-exports $download |
        ForEach-Object {
            if ($_ -match "^\s+Name:\s+(.+)$") { $Matches[1] }
        }
    $required = Get-Content -LiteralPath (Join-Path $root "crates/cronet-sys/abi/cronet-go-d62042e.symbols") |
        Where-Object { $_ -and -not $_.StartsWith("#") }
    $missing = $required | Where-Object { $_ -notin $exported }
    if ($missing) {
        throw "Native DLL is missing required exports: $($missing -join ', ')"
    }
}

$runtime = Join-Path $bin "cronet.dll"
Copy-Item -LiteralPath $download -Destination $runtime -Force
$symbols = Join-Path $root "crates/cronet-sys/abi/cronet-go-d62042e.symbols"
$definition = Join-Path $lib "cronet.def"
$definitionLines = @("LIBRARY cronet.dll", "EXPORTS")
$definitionLines += Get-Content -LiteralPath $symbols |
    Where-Object { $_ -and -not $_.StartsWith("#") } |
    ForEach-Object { "    $($_.Trim())" }
Set-Content -LiteralPath $definition -Value $definitionLines -Encoding ascii

$dlltool = Get-Command llvm-dlltool -ErrorAction SilentlyContinue
if (-not $dlltool) {
    $dlltool = Get-Command dlltool -ErrorAction SilentlyContinue
}
if (-not $dlltool) {
    throw "llvm-dlltool or dlltool is required to create cronet.lib"
}
$machine = if ($Architecture -eq "arm64") { "arm64" } else { "i386:x86-64" }
& $dlltool.Source -m $machine -d $definition -l (Join-Path $lib "cronet.lib")
if ($LASTEXITCODE -ne 0) {
    throw "Import-library generation failed with exit code $LASTEXITCODE"
}

Write-Output "Verified $asset ($actual)"
Write-Output "CRONET_LIB_DIR=$lib"
Write-Output "CRONET_LIB_NAME=cronet"
Write-Output "Add to PATH: $bin"
