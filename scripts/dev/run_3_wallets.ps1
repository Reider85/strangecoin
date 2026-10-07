# Скрипт запуска трёх чистых кошельков (инстансов приложения) на одной машине.
# Каждый запуск полностью очищает состояние: удаляет БД и кошельки инстансов.
# Каждый инстанс работает в своей папке instances\wallet_<port> с собственным config.json,
# network.json и базой данных blockchain_db_<port>.
#
# Запуск:  powershell -ExecutionPolicy Bypass -File .\run_3_wallets.ps1

$ErrorActionPreference = "Stop"

$ports = @(8081, 8082, 8083)
$exeName = "strangecoin.exe"
$processName = "strangecoin"
$projectRoot = $PSScriptRoot
$exePath = Join-Path $projectRoot "target\debug\$exeName"

# Сборка, если исполняемый файл отсутствует
if (-not (Test-Path -LiteralPath $exePath)) {
    Write-Host "Исполняемый файл не найден, выполняю сборку..." -ForegroundColor Yellow
    & "$env:USERPROFILE\.cargo\bin\cargo.exe" build
    if (-not (Test-Path -LiteralPath $exePath)) {
        throw "Сборка не удалась: не найден $exePath"
    }
}

$instancesRoot = Join-Path $projectRoot "instances"
New-Item -ItemType Directory -Path $instancesRoot -Force | Out-Null

$utf8NoBom = New-Object System.Text.UTF8Encoding($false)

# Подготовка чистых директорий для каждого инстанса
foreach ($port in $ports) {
    $dir = Join-Path $instancesRoot "wallet_$port"

    # Останавливаем процессы этого инстанса
    Get-Process -Name $processName -ErrorAction SilentlyContinue |
        Where-Object { $_.Path -like "$dir*" } |
        Stop-Process -Force -ErrorAction SilentlyContinue

    # Удаляем старые данные (БД, кошельки, конфиги) и создаём чистую директорию
    if (Test-Path -LiteralPath $dir) {
        Remove-Item -LiteralPath $dir -Recurse -Force
    }
    New-Item -ItemType Directory -Path $dir -Force | Out-Null

    # Копируем исполняемый файл
    Copy-Item -LiteralPath $exePath -Destination (Join-Path $dir $exeName)

    # Чистый config.json без кошелька
    $config = @{ wallet = @{ name = ""; password = ""; port = $port; ip = "127.0.0.1" } }
    [System.IO.File]::WriteAllText(
        (Join-Path $dir "config.json"),
        ($config | ConvertTo-Json -Depth 3),
        $utf8NoBom
    )

    # network.json со всеми тремя пирами
    $network = @{ peers = @("127.0.0.1:8081", "127.0.0.1:8082", "127.0.0.1:8083") }
    [System.IO.File]::WriteAllText(
        (Join-Path $dir "network.json"),
        ($network | ConvertTo-Json -Depth 3),
        $utf8NoBom
    )

    Write-Host "Подготовлен чистый инстанс: $dir"
}

# Запуск трёх инстансов, каждому задаётся свой PORT
foreach ($port in $ports) {
    $exe = Join-Path $instancesRoot "wallet_$port\$exeName"
    $env:PORT = "$port"
    Write-Host "Запуск чистого кошелька на порту $port..." -ForegroundColor Green
    Start-Process -FilePath $exe -WorkingDirectory (Join-Path $instancesRoot "wallet_$port")
}

Write-Host "Все три чистых кошелька запущены (порты: $($ports -join ', '))." -ForegroundColor Green
