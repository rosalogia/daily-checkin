#!/bin/bash

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SERVICE_NAME="daily-checkin-bot"
SERVICE_FILE="/etc/systemd/system/${SERVICE_NAME}.service"
SERVICE_TEMPLATE="${SCRIPT_DIR}/${SERVICE_NAME}.service"
ENV_FILE="/etc/default/${SERVICE_NAME}"

echo "Daily Check-in Bot Installation Script"
echo "====================================="

# Check if running as root
if [[ $EUID -eq 0 ]]; then
   echo "This script should not be run as root. Please run as a regular user with sudo privileges."
   exit 1
fi

# Check if systemd is available
if ! command -v systemctl &> /dev/null; then
    echo "Error: systemd is not available on this system."
    exit 1
fi

# Check if the binary exists
BINARY_PATH="${SCRIPT_DIR}/target/release/daily-checkin-bot"
if [[ ! -f "$BINARY_PATH" ]]; then
    echo "Error: Binary not found at $BINARY_PATH"
    echo "Please build the project first with: cargo build --release"
    exit 1
fi

# Check if service template exists
if [[ ! -f "$SERVICE_TEMPLATE" ]]; then
    echo "Error: Service template not found at $SERVICE_TEMPLATE"
    exit 1
fi

# Prompt for Discord token
echo
read -s -p "Enter your Discord bot token: " DISCORD_TOKEN
echo
if [[ -z "$DISCORD_TOKEN" ]]; then
    echo "Error: Discord token cannot be empty."
    exit 1
fi

# Validate token format (basic check)
if [[ ! $DISCORD_TOKEN =~ ^[A-Za-z0-9._-]{24}\.[A-Za-z0-9._-]{6}\.[A-Za-z0-9._-]{27}$ ]]; then
    echo "Warning: The provided token doesn't match the expected Discord bot token format."
    read -p "Continue anyway? (y/N): " confirm
    if [[ $confirm != [yY] ]]; then
        echo "Installation cancelled."
        exit 1
    fi
fi

# Prompt for optional RUST_LOG level
echo
echo "Log level options: error, warn, info, debug, trace"
read -p "Enter log level (default: info): " RUST_LOG
RUST_LOG=${RUST_LOG:-"daily_checkin_bot=info,serenity=warn"}

# Create user for the service
echo
echo "Creating system user for the service..."
if ! id "$SERVICE_NAME" &>/dev/null; then
    sudo useradd --system --shell /bin/false --home-dir /var/lib/$SERVICE_NAME --create-home $SERVICE_NAME
    echo "Created user: $SERVICE_NAME"
else
    echo "User $SERVICE_NAME already exists."
fi

# Create directories and set permissions
echo "Setting up directories and permissions..."
sudo mkdir -p /var/lib/$SERVICE_NAME
sudo mkdir -p /var/log/$SERVICE_NAME

# Copy the binary
echo "Installing binary..."
sudo cp "$BINARY_PATH" /usr/local/bin/
sudo chmod +x /usr/local/bin/daily-checkin-bot
sudo chown root:root /usr/local/bin/daily-checkin-bot

# Copy bot data if it exists
if [[ -f "$SCRIPT_DIR/bot_data.json" ]]; then
    echo "Copying existing bot data..."
    sudo cp "$SCRIPT_DIR/bot_data.json" /var/lib/$SERVICE_NAME/
    sudo chown $SERVICE_NAME:$SERVICE_NAME /var/lib/$SERVICE_NAME/bot_data.json
fi

# Set ownership
sudo chown -R $SERVICE_NAME:$SERVICE_NAME /var/lib/$SERVICE_NAME
sudo chown -R $SERVICE_NAME:$SERVICE_NAME /var/log/$SERVICE_NAME

# Create environment file
echo "Creating environment configuration..."
sudo tee "$ENV_FILE" > /dev/null <<EOF
# Discord bot token
DISCORD_TOKEN=$DISCORD_TOKEN

# Logging configuration
RUST_LOG=$RUST_LOG
EOF

sudo chmod 600 "$ENV_FILE"
sudo chown root:root "$ENV_FILE"

# Install systemd service file
echo "Installing systemd service..."
sudo cp "$SERVICE_TEMPLATE" "$SERVICE_FILE"
sudo chmod 644 "$SERVICE_FILE"
sudo chown root:root "$SERVICE_FILE"

# Reload systemd and enable service
echo "Configuring systemd service..."
sudo systemctl daemon-reload
sudo systemctl enable $SERVICE_NAME

echo
echo "Installation completed successfully!"
echo
echo "To manage the service, use:"
echo "  Start:   sudo systemctl start $SERVICE_NAME"
echo "  Stop:    sudo systemctl stop $SERVICE_NAME"
echo "  Status:  sudo systemctl status $SERVICE_NAME"
echo "  Logs:    sudo journalctl -u $SERVICE_NAME -f"
echo
echo "The service is configured to start automatically on boot."
echo "Configuration files:"
echo "  Service file: $SERVICE_FILE"
echo "  Environment:  $ENV_FILE"
echo "  Working dir:  /var/lib/$SERVICE_NAME"
echo
read -p "Start the service now? (Y/n): " start_now
if [[ $start_now != [nN] ]]; then
    echo "Starting service..."
    sudo systemctl start $SERVICE_NAME
    sleep 2
    sudo systemctl status $SERVICE_NAME --no-pager
fi