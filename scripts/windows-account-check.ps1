param(
    [Parameter(Mandatory=$true)][string]$PrivateReference,
    [Parameter(Mandatory=$true)][string]$ExpectedOwnerSid,
    [Parameter(Mandatory=$true)][ValidateSet('owner', 'outsider')][string]$Role,
    [Parameter(Mandatory=$true)][ValidateRange(11420,11429)][int]$ApiPort,
    [Parameter(Mandatory=$true)][Guid]$RunId,
    [Parameter(Mandatory=$true)][string]$OutFile
)
$ErrorActionPreference = 'Stop'
$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$sid = $identity.User.Value
$adminMember = @($identity.Groups | ForEach-Object { $_.Value }) -contains 'S-1-5-32-544'
if ($adminMember -or ($Role -eq 'owner' -and $sid -ne $ExpectedOwnerSid) -or
    ($Role -eq 'outsider' -and $sid -eq $ExpectedOwnerSid)) {
    throw 'Use the declared owner and a distinct ordinary non-administrator outsider account.'
}
$root = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$tmp = [IO.Path]::GetFullPath((Join-Path $root 'tmp')) + [IO.Path]::DirectorySeparatorChar
$output = [IO.Path]::GetFullPath($OutFile)
if (-not $output.StartsWith($tmp, [StringComparison]::OrdinalIgnoreCase) -or [IO.File]::Exists($output)) {
    throw 'Write a fresh nonsecret receipt under this checkout tmp directory.'
}
$referencePath = [IO.Path]::GetFullPath($PrivateReference)
$hash = [Security.Cryptography.SHA256]::Create()
$pathHash = ([BitConverter]::ToString($hash.ComputeHash([Text.Encoding]::UTF8.GetBytes($referencePath.ToUpperInvariant())))).Replace('-', '').ToLowerInvariant()
$hash.Dispose()
$readable = $false
$denial = $null
$reference = $null
try {
    $stream = [IO.File]::OpenRead($referencePath)
    try {
        $readable = $true
        if ($Role -eq 'outsider') { throw 'Private bytes were accessible to the outsider; qualification failed.' }
        if ($stream.Length -gt 1024) { throw 'Unexpected private reference size.' }
        $reader = [IO.StreamReader]::new($stream, [Text.Encoding]::UTF8, $true, 1024, $true)
        try {
            try { $reference = $reader.ReadToEnd() | ConvertFrom-Json }
            catch { throw 'Private reference is invalid; no bytes retained.' }
        } finally { $reader.Dispose() }
    } finally { $stream.Dispose() }
} catch [UnauthorizedAccessException] {
    $denial = 'UnauthorizedAccess'
}
if ($Role -eq 'owner' -and -not $readable) { throw 'Owner positive-control private read failed.' }
if ($Role -eq 'outsider' -and ($readable -or $denial -ne 'UnauthorizedAccess')) { throw 'Outsider read did not prove access denial.' }
if ($Role -eq 'owner' -and ($reference.port -ne $ApiPort -or $reference.token -notmatch '^colima-[a-f0-9]{64}$')) {
    throw 'Owner reference does not match the exact arranged app port.'
}
function Read-Status([string]$Path, [hashtable]$Headers) {
    try {
        $response = Invoke-WebRequest -UseBasicParsing -Uri "http://127.0.0.1:$ApiPort$Path" -Headers $Headers -TimeoutSec 5
        return [int]$response.StatusCode
    } catch {
        if ($null -eq $_.Exception.Response) { throw 'Exact arranged API unavailable; no port fallback.' }
        return [int]$_.Exception.Response.StatusCode
    }
}
$health = Read-Status '/api/health' @{}
$headers = @{}
if ($Role -eq 'owner') { $headers['Authorization'] = 'Bearer ' + $reference.token }
$protected = Read-Status '/api/containers' $headers
$headers.Clear()
$reference = $null
if ($health -ne 200 -or ($Role -eq 'owner' -and $protected -ne 200) -or
    ($Role -eq 'outsider' -and $protected -ne 401)) { throw 'Public/owner/outsider API control failed.' }
$receipt = @{
    runId = $RunId.ToString(); role = $Role; accountSid = $sid; ownerSid = $ExpectedOwnerSid
    administratorMember = $adminMember; referencePathSha256 = $pathHash
    privateReadable = $readable; privateDenial = $denial; port = $ApiPort
    healthStatus = $health; protectedStatus = $protected
    result = 'PASS'; fullQualification = 'PENDING - creation/privacy failure and resource preservation variants are separate'
}
[IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($output)) | Out-Null
# Credentials/private bytes are never serialized, including on refusal.
[IO.File]::WriteAllText($output, ($receipt | ConvertTo-Json -Depth 4), [Text.UTF8Encoding]::new($false))
Write-Output 'Account boundary assertions recorded; no credential bytes were logged.'
