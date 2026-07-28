# portal-auto-login: Auto login script for portal authentication
# Login flow: Visit portal page to get Cookie -> RC4 encrypt password -> POST login request

param(
    [string]$ConfigFile = ""
)

# ========== RC4 Encryption (same as do_encrypt_rc4 on web) ==========
function Invoke-RC4Encrypt {
    param([string]$PlainText, [string]$Key)

    $src = $PlainText
    $passwd = $Key

    $sbox = New-Object int[] 256
    $keyArr = New-Object int[] 256

    for ($i = 0; $i -lt 256; $i++) {
        $keyArr[$i] = [int][char]$passwd[$i % $passwd.Length]
        $sbox[$i] = $i
    }

    $j = 0
    for ($i = 0; $i -lt 256; $i++) {
        $j = ($j + $sbox[$i] + $keyArr[$i]) % 256
        $temp = $sbox[$i]
        $sbox[$i] = $sbox[$j]
        $sbox[$j] = $temp
    }

    $a = 0
    $b = 0
    $output = @()
    for ($i = 0; $i -lt $src.Length; $i++) {
        $a = ($a + 1) % 256
        $b = ($b + $sbox[$a]) % 256
        $temp = $sbox[$a]
        $sbox[$a] = $sbox[$b]
        $sbox[$b] = $temp
        $c = ($sbox[$a] + $sbox[$b]) % 256
        $xorVal = [int][char]$src[$i] -bxor $sbox[$c]
        $hex = $xorVal.ToString("x2")
        $output += $hex
    }

    return ($output -join "")
}

# ========== Read Config File ==========
function Read-Config {
    param([string]$FilePath)

    if (-not (Test-Path $FilePath)) {
        Write-Host "[FAIL] Config file not found: $FilePath"
        return $null
    }

    $config = @{}
    $currentSection = ""

    foreach ($line in Get-Content $FilePath -Encoding UTF8) {
        $trimmedLine = $line.Trim()
        if ([string]::IsNullOrWhiteSpace($trimmedLine) -or $trimmedLine.StartsWith(";") -or $trimmedLine.StartsWith("#")) {
            continue
        }
        if ($trimmedLine -match '^\[(.+)\]$') {
            $currentSection = $Matches[1]
            continue
        }
        # Match key=value, value may contain special chars like @ * !
        # Inline comment starts with space+; or space+# after the value
        if ($trimmedLine -match '^([^=]+?)\s*=\s*(.+)$') {
            $key = $Matches[1].Trim()
            $rawValue = $Matches[2]
            # Strip inline comment: only if there is a space before ; or #
            # This prevents cutting passwords that contain ; or # without preceding space
            if ($rawValue -match '^(.+?)\s+[;#]') {
                $rawValue = $Matches[1].TrimEnd()
            }
            # Remove surrounding double quotes
            $value = $rawValue.Trim()
            if ($value.Length -ge 2 -and $value.StartsWith('"') -and $value.EndsWith('"')) {
                $value = $value.Substring(1, $value.Length - 2)
            }
            $fullKey = if ($currentSection) { "${currentSection}_${key}" } else { $key }
            $config[$fullKey] = $value
        }
    }

    return $config
}

# ========== Check if already logged in ==========
function Test-AlreadyLoggedIn {
    param([string]$HostUrl)

    try {
        $response = Invoke-WebRequest -Uri "$HostUrl/homepage/index.html" -Method GET -UseBasicParsing -TimeoutSec 10 -MaximumRedirection 0 -ErrorAction SilentlyContinue
        if ($response.StatusCode -eq 200 -and $response.Content -notmatch "ac_portal") {
            return $true
        }
    } catch {
        if ($_.Exception.Response) {
            $statusCode = [int]$_.Exception.Response.StatusCode
            if ($statusCode -eq 302 -or $statusCode -eq 301) {
                return $false
            }
        }
    }

    try {
        $response = Invoke-WebRequest -Uri "$HostUrl/homepage/info.php" -Method POST -Body "opr=list" -ContentType "application/x-www-form-urlencoded; charset=UTF-8" -UseBasicParsing -TimeoutSec 10 -ErrorAction Stop
        $json = $response.Content | ConvertFrom-Json
        if ($json.success -eq $true) {
            return $true
        }
    } catch {
        # Request failed means not logged in
    }

    return $false
}

# ========== Perform Login ==========
# Returns: 0 = success, 1 = failure
# Uses script-level variable to avoid mixing output with Write-Host
$script:loginResult = 1

function Invoke-PortalLogin {
    param([hashtable]$Config)

    $hostUrl = $Config["portal_HOST"]
    $loginUrl = $Config["portal_LOGIN_URL"]
    $portalUrl = $Config["portal_PORTAL_URL"]
    $username = $Config["account_USERNAME"]
    $password = $Config["account_PASSWORD"]
    $rememberPwd = $Config["account_REMEMBER_PWD"]

    if (-not $loginUrl.StartsWith("http")) {
        $loginUrl = "$hostUrl$loginUrl"
    }
    if (-not $portalUrl.StartsWith("http")) {
        $portalUrl = "$hostUrl$portalUrl"
    }

    # Step 1: Visit portal page to get initial Cookie (Session)
    Write-Host "[INFO] Visiting portal page to get Cookie..."
    $session = New-Object Microsoft.PowerShell.Commands.WebRequestSession
    try {
        $null = Invoke-WebRequest -Uri $portalUrl -Method GET -WebSession $session -UseBasicParsing -TimeoutSec 15 -ErrorAction Stop
        Write-Host "[INFO] Cookie obtained successfully"
    } catch {
        Write-Host "[WARN] Failed to visit portal page: $($_.Exception.Message)"
        Write-Host "[INFO] Trying to send login request directly..."
    }

    # Step 2: Generate RC4 key (timestamp) and encrypt password
    $rckey = [string]([long]((Get-Date) - (Get-Date "1970-01-01")).TotalMilliseconds)
    $encryptedPwd = Invoke-RC4Encrypt -PlainText $password -Key $rckey

    Write-Host "[INFO] Encrypting password with RC4 (key length: $($rckey.Length))"

    # Step 3: Build login request body as string (to control encoding)
    $body = "opr=pwdLogin&userName=" + [System.Uri]::EscapeDataString($username) + "&pwd=" + $encryptedPwd + "&auth_tag=" + $rckey + "&rememberPwd=" + $(if ($rememberPwd -eq "1") { "1" } else { "0" })

    Write-Host "[INFO] Sending login request..."

    # Step 4: Send login request
    try {
        $response = Invoke-WebRequest -Uri $loginUrl -Method POST -Body $body -ContentType "application/x-www-form-urlencoded; charset=UTF-8" -WebSession $session -UseBasicParsing -TimeoutSec 15 -ErrorAction Stop

        $json = $response.Content | ConvertFrom-Json

        if ($json.success -eq $true) {
            Write-Host "[INFO] Server accepted login"

            # Step 5: Visit location URL to complete authentication
            if ($json.location -and $json.location.Length -gt 0) {
                $locationUrl = $json.location
                # Normalize URL: remove explicit :80 port
                $locationUrl = $locationUrl -replace '^(http://[^/:]+):80(/)', '$1$2'
                Write-Host "[INFO] Visiting location URL to finalize auth..."
                try {
                    $null = Invoke-WebRequest -Uri $locationUrl -Method GET -WebSession $session -UseBasicParsing -TimeoutSec 15 -ErrorAction Stop
                    Write-Host "[INFO] Location URL visited"
                } catch {
                    Write-Host "[WARN] Failed to visit location URL: $($_.Exception.Message)"
                }
            }

            # Step 6: Verify login via info.php
            try {
                $verifyResp = Invoke-WebRequest -Uri "$hostUrl/homepage/info.php" -Method POST -Body "opr=list" -ContentType "application/x-www-form-urlencoded; charset=UTF-8" -WebSession $session -UseBasicParsing -TimeoutSec 10 -ErrorAction Stop
                $verifyJson = $verifyResp.Content | ConvertFrom-Json
                if ($verifyJson.success -eq $true) {
                    Write-Host "[SUCCESS] Login verified! User: $($verifyJson.data.basic.name)"
                    $script:loginResult = 0
                    return
                } else {
                    Write-Host "[WARN] info.php check returned unexpected result"
                }
            } catch {
                Write-Host "[WARN] info.php verification failed: $($_.Exception.Message)"
            }

            # Even if info.php check fails, server said success
            Write-Host "[SUCCESS] Login request accepted by server!"
            if ($json.msg) {
                Write-Host "[INFO] Server message: $($json.msg)"
            }
            $script:loginResult = 0
            return
        } else {
            Write-Host "[FAIL] Login failed: $($json.msg)"
            $script:loginResult = 1
            return
        }
    } catch {
        if ($_.Exception.Response) {
            $statusCode = [int]$_.Exception.Response.StatusCode
            Write-Host "[WARN] HTTP status code: $statusCode"
        }
        Write-Host "[FAIL] Login request error: $($_.Exception.Message)"
        $script:loginResult = 1
        return
    }
}

# ========== Main Flow ==========
if ([string]::IsNullOrEmpty($ConfigFile)) {
    $scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
    $ConfigFile = Join-Path $scriptDir "config.ini"
}

Write-Host "=========================================="
Write-Host "  Portal Auto Login v1.0"
Write-Host "=========================================="
Write-Host "[INFO] Config file: $ConfigFile"
Write-Host ""

$config = Read-Config -FilePath $ConfigFile
if ($null -eq $config) {
    Write-Host "[FAIL] Cannot read config file"
    exit 1
}

$requiredKeys = @("portal_HOST", "portal_LOGIN_URL", "account_USERNAME", "account_PASSWORD")
foreach ($key in $requiredKeys) {
    if (-not $config.ContainsKey($key) -or [string]::IsNullOrEmpty($config[$key])) {
        Write-Host "[FAIL] Missing config key: $key"
        exit 1
    }
}


$retryCount = if ($config.ContainsKey("settings_RETRY_COUNT")) { [int]$config["settings_RETRY_COUNT"] } else { 3 }
$retryInterval = if ($config.ContainsKey("settings_RETRY_INTERVAL")) { [int]$config["settings_RETRY_INTERVAL"] } else { 5 }

Write-Host "[INFO] Checking login status..."
$alreadyLoggedIn = Test-AlreadyLoggedIn -HostUrl $config["portal_HOST"]
if ($alreadyLoggedIn) {
    Write-Host "[INFO] Already logged in, no need to authenticate again"
    exit 0
}
Write-Host "[INFO] Not logged in, starting authentication..."
Write-Host ""

$attempt = 0
$success = $false
while ($attempt -lt $retryCount) {
    $attempt++
    Write-Host "[INFO] Attempt $attempt/$retryCount..."

    $script:loginResult = 1
    Invoke-PortalLogin -Config $config

    if ($script:loginResult -eq 0) {
        $success = $true
        break
    }

    if ($attempt -lt $retryCount) {
        Write-Host "[INFO] Retrying in $retryInterval seconds..."
        Start-Sleep -Seconds $retryInterval
    }
}

if ($success) {
    Write-Host ""
    Write-Host "=========================================="
    Write-Host "  Login Successful!"
    Write-Host "=========================================="
    exit 0
} else {
    Write-Host ""
    Write-Host "=========================================="
    Write-Host "  Login Failed after $retryCount attempts"
    Write-Host "=========================================="
    exit 1
}
