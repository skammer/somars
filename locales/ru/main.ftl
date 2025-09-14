# General UI
app-title = Плеер SomaFM
app-description = Плеер soma.fm на Rust

# Main UI
stations = Станции
history = История
loading = Загрузка
loading-stations = Загрузка станций...
no-station-selected = Станция не выбрана
volume = Громкость
playback-time = Время воспроизведения

# Station info
station-id = ID
station-title = Название
station-genre = Жанр
station-dj = Диджей

# Playback states
playing = Воспроизведение
paused = Пауза
stopped = Остановлено

# Controls
controls-quit = Выход
controls-play = Играть
controls-stop = Стоп
controls-start = Старт
controls-pause = Пауза
controls-volume = Громкость
controls-help = Помощь

# Messages
connecting-to-stream = Подключение к потоку...
playback-started = Воспроизведение начато
playback-error = Ошибка воспроизведения: {$error}
starting-playback = Начинаем воспроизведение {$station}
failed-playback = Не удалось начать воспроизведение
failed-audio-sink = Не удалось получить доступ к аудиоустройству
stream-from = Инициализация потока из: {$url}
got-response = Получен ответ, запуск потока...
bit-rate = Битрейт: {$rate}кбит/с
udp-starting = Запуск UDP-слушателя на порту {$port}
udp-error = Ошибка UDP: {$error}
station-not-found = Станция с ID не найдена: {$id}
auto-playing = Автоматическое воспроизведение станции: {$id}

# Help screen
help-title = Справка
help-keyboard = Управление с клавиатуры
help-enter = Воспроизвести выбранную станцию
help-space = Остановить/Начать воспроизведение
help-volume = Регулировка громкости
help-arrows = Навигация по станциям
help-quit = Выйти из приложения
help-toggle-help = Показать/скрыть эту справку
help-cli = Аргументы командной строки
help-log-level = Уровень логирования (1=минимальный, 2=подробный)
help-station = Автоматически воспроизводить станцию при запуске
help-listen = Включить UDP-управление
help-port = Установить UDP-порт (по умолчанию: 8069)
help-show-help = Показать справку по командной строке
help-version = Показать информацию о версии
help-broadcast = Отправить UDP-команду в сеть и выйти
help-locale = Установить язык (en, ru)
help-close = Нажмите ? чтобы закрыть эту справку
