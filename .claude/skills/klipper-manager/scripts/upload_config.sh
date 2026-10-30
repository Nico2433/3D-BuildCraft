#!/bin/bash
# upload_config.sh — Upload a config file to Moonraker, backing up the current one first
# Usage: bash upload_config.sh <moonraker_ip> <local_config_file> [options]
#
# Options:
#   --name <file>   Remote file name in the config root (default: printer.cfg)
#   --no-restart    Upload only; do not run FIRMWARE_RESTART
#   --force         Upload even if the local file would drop the remote SAVE_CONFIG block
#
# Before uploading it:
#   1. refuses while a print is running or paused,
#   2. saves the current remote file locally (./config_backups/<ip>/<name>.<timestamp>)
#      and as <name>.bak-<timestamp> next to it on the printer,
#   3. refuses (without --force) if the remote file has a SAVE_CONFIG block and the
#      local file does not, since that would erase the saved calibration.
#
# Exit codes: 0=success, 1=usage/connection error, 2=upload error, 3=restart error,
#             4=refused (printing, or SAVE_CONFIG block would be lost)

set -euo pipefail

IP="${1:?Usage: upload_config.sh <moonraker_ip> <local_config_file> [--name F] [--no-restart] [--force]}"
LOCAL_FILE="${2:?Provide path to local config file}"
shift 2
NAME="printer.cfg"
RESTART=1
FORCE=0
while [ $# -gt 0 ]; do
  case "$1" in
    --name) NAME="${2:?--name needs a file name}"; shift 2 ;;
    --no-restart) RESTART=0; shift ;;
    --force) FORCE=1; shift ;;
    *) echo "ERROR: unknown option: $1" >&2; exit 1 ;;
  esac
done

URL="http://${IP}:7125"
CURL=(curl -sS --connect-timeout 5 --max-time 60)
SAVE_MARK='#*# <---------------------- SAVE_CONFIG ---------------------->'

json_get() {  # json_get <python expression on d> — reads JSON from stdin
  python3 -c "import sys,json; d=json.load(sys.stdin); print($1)" 2>/dev/null || true
}

if [ ! -f "$LOCAL_FILE" ]; then
  echo "ERROR: File not found: ${LOCAL_FILE}" >&2
  exit 1
fi

# 1. Connectivity and print state
INFO=$("${CURL[@]}" "${URL}/printer/info" || true)
if [ -z "$INFO" ]; then
  echo "ERROR: Cannot connect to Moonraker at ${URL}" >&2
  exit 1
fi
PRINT_STATE=$("${CURL[@]}" "${URL}/printer/objects/query?print_stats=state" \
  | json_get "d['result']['status']['print_stats']['state']" || true)
if [ "$PRINT_STATE" = "printing" ] || [ "$PRINT_STATE" = "paused" ]; then
  echo "ERROR: printer is ${PRINT_STATE}; refusing to change config during a print" >&2
  exit 4
fi

# 2. Back up the current remote file (if it exists)
STAMP="$(date +%Y%m%d_%H%M%S)_$$"
BACKUP_DIR="./config_backups/${IP}"
mkdir -p "$BACKUP_DIR"
LOCAL_BACKUP="${BACKUP_DIR}/${NAME//\//_}.${STAMP}"
HTTP=$("${CURL[@]}" -o "$LOCAL_BACKUP" -w "%{http_code}" "${URL}/server/files/config/${NAME}" || true)
if [ "$HTTP" = "200" ]; then
  echo "Backed up remote ${NAME} to ${LOCAL_BACKUP}" >&2
  COPY=$("${CURL[@]}" -X POST "${URL}/server/files/copy" -H "Content-Type: application/json" \
    -d "{\"source\": \"config/${NAME}\", \"dest\": \"config/${NAME}.bak-${STAMP}\"}" || true)
  if [ -n "$(echo "$COPY" | json_get "d.get('error',{}).get('message','')")" ] || [ -z "$COPY" ]; then
    echo "WARNING: could not copy ${NAME} on the printer; the local backup is the only one" >&2
  else
    echo "Backed up on the printer as ${NAME}.bak-${STAMP}" >&2
  fi
  # 3. SAVE_CONFIG guard
  if grep -qF "$SAVE_MARK" "$LOCAL_BACKUP" && ! grep -qF "$SAVE_MARK" "$LOCAL_FILE" && [ "$FORCE" -eq 0 ]; then
    echo "ERROR: remote ${NAME} has a SAVE_CONFIG block (saved calibration) and the local file" >&2
    echo "       does not. Copy the block from ${LOCAL_BACKUP} or pass --force." >&2
    exit 4
  fi
elif [ "$HTTP" = "404" ]; then
  rm -f "$LOCAL_BACKUP"
  echo "Remote ${NAME} does not exist yet; nothing to back up" >&2
else
  rm -f "$LOCAL_BACKUP"
  echo "ERROR: could not read remote ${NAME} for backup (HTTP ${HTTP}); not uploading" >&2
  exit 1
fi

# 4. Upload
echo "Uploading $(basename "$LOCAL_FILE") as ${NAME} to ${URL}..." >&2
HTTP=$("${CURL[@]}" -o /dev/null -w "%{http_code}" -X POST "${URL}/server/files/upload" \
  -F "root=config" -F "file=@${LOCAL_FILE};filename=${NAME}" || true)
if [ "$HTTP" != "201" ] && [ "$HTTP" != "200" ]; then
  echo "ERROR: Upload failed with HTTP ${HTTP}" >&2
  exit 2
fi
echo "Upload successful (HTTP ${HTTP})" >&2

if [ "$RESTART" -eq 0 ]; then
  echo "Skipping firmware restart (--no-restart)" >&2
  exit 0
fi

# 5. Firmware restart and wait for ready (up to 60 s)
echo "Restarting firmware..." >&2
"${CURL[@]}" -X POST "${URL}/printer/firmware_restart" > /dev/null || true
for i in $(seq 1 30); do
  sleep 2
  INFO=$("${CURL[@]}" "${URL}/printer/info" || true)
  STATE=$(echo "$INFO" | json_get "d['result']['state']")
  echo "  [${i}] state: ${STATE:-no answer}" >&2
  case "$STATE" in
    ready) echo "Klipper is ready." >&2; exit 0 ;;
    error|shutdown)
      echo "ERROR: Klipper entered ${STATE} state after restart" >&2
      echo "Message: $(echo "$INFO" | json_get "d['result']['state_message']")" >&2
      [ -f "${LOCAL_BACKUP:-}" ] && echo "Restore with: bash $0 ${IP} ${LOCAL_BACKUP} --name ${NAME} --force" >&2
      exit 3 ;;
  esac
done

echo "WARNING: Klipper did not reach ready state within 60s" >&2
exit 3
