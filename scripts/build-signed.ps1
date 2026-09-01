$ErrorActionPreference = 'Stop'

$keyPath = 'C:\Users\CRH38\.tauri\amdl.key'
if (-not (Test-Path -LiteralPath $keyPath -PathType Leaf)) {
    throw "Tauri signing private key not found: $keyPath"
}

$env:TAURI_SIGNING_PRIVATE_KEY = $keyPath
$passwordWasProvided = -not [string]::IsNullOrEmpty($env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD)

try {
    if (-not $passwordWasProvided) {
        $securePassword = Read-Host 'Enter Tauri signing private key password' -AsSecureString
        $passwordPointer = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($securePassword)
        try {
            $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = [Runtime.InteropServices.Marshal]::PtrToStringBSTR($passwordPointer)
        }
        finally {
            [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($passwordPointer)
        }
    }

    & pnpm exec tauri build
    if ($LASTEXITCODE -ne 0) {
        throw "Tauri signed build failed with exit code $LASTEXITCODE"
    }
}
finally {
    Remove-Item Env:TAURI_SIGNING_PRIVATE_KEY -ErrorAction SilentlyContinue
    if (-not $passwordWasProvided) {
        Remove-Item Env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD -ErrorAction SilentlyContinue
    }
}
