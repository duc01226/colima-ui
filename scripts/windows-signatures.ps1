param(
    [Parameter(Mandatory=$true)][string]$Application,
    [Parameter(Mandatory=$true)][string]$Installer,
    [Parameter(Mandatory=$true)][string]$Thumbprint
)
$ErrorActionPreference = 'Stop'
$records = @()
foreach ($artifact in @($Application, $Installer)) {
    $signature = Get-AuthenticodeSignature -LiteralPath $artifact
    if ($signature.Status -ne 'Valid' -or
        $signature.SignerCertificate.Thumbprint -ne $Thumbprint -or
        $null -eq $signature.TimeStamperCertificate) {
        throw 'Expected trusted, timestamped Authenticode signature from the configured certificate.'
    }
    $records += @{
        fileName = [IO.Path]::GetFileName($artifact)
        sha256 = (Get-FileHash -LiteralPath $artifact -Algorithm SHA256).Hash.ToLowerInvariant()
        signerThumbprint = $signature.SignerCertificate.Thumbprint
        timestampThumbprint = $signature.TimeStamperCertificate.Thumbprint
        status = $signature.Status.ToString()
    }
}
ConvertTo-Json -InputObject $records -Depth 4 -Compress
