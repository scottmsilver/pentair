#!/usr/bin/env bash
set -euo pipefail

BASE_URL="${BASE_URL:-http://127.0.0.1:8083}"
OUT_DIR="${OUT_DIR:-/home/ssilver/development/pentair/docs/experiments/spa-heat-runs/2026-03-23-90f}"
TARGET_TEMP="${TARGET_TEMP:-90}"
RESTORE_SETPOINT="${RESTORE_SETPOINT:-100}"
RESTORE_MODE="${RESTORE_MODE:-heat-pump}"
TIMEOUT_SECONDS="${TIMEOUT_SECONDS:-3600}"
SAMPLE_SECONDS="${SAMPLE_SECONDS:-30}"

SAMPLES_FILE="${OUT_DIR}/samples.jsonl"
SUMMARY_FILE="${OUT_DIR}/summary.json"

mkdir -p "${OUT_DIR}"
: >"${SAMPLES_FILE}"

cleanup() {
  curl -sS -m 5 -X POST "${BASE_URL}/api/spa/off" >/dev/null || true
  curl -sS -m 5 -X POST "${BASE_URL}/api/spa/heat" \
    -H 'content-type: application/json' \
    -d "{\"setpoint\":${RESTORE_SETPOINT},\"mode\":\"${RESTORE_MODE}\"}" >/dev/null || true
}

trap cleanup EXIT

capture_sample() {
  local phase="$1"
  local elapsed="$2"
  curl -sS -m 5 "${BASE_URL}/api/pool" \
    | jq -c \
        --arg captured_at "$(date -Iseconds)" \
        --arg phase "${phase}" \
        --argjson elapsed_seconds "${elapsed}" \
        --argjson target_temperature "${TARGET_TEMP}" \
        '{captured_at:$captured_at, phase:$phase, elapsed_seconds:$elapsed_seconds, target_temperature:$target_temperature, spa:.spa, pool:.pool, system:.system}'
}

capture_sample "before" 0 >>"${SAMPLES_FILE}"

curl -sS -m 5 -X POST "${BASE_URL}/api/spa/heat" \
  -H 'content-type: application/json' \
  -d "{\"setpoint\":${TARGET_TEMP},\"mode\":\"heat-pump\"}" >/dev/null
curl -sS -m 5 -X POST "${BASE_URL}/api/spa/on" >/dev/null

start_epoch="$(date +%s)"
deadline=$((start_epoch + TIMEOUT_SECONDS))
reached=0

while true; do
  now="$(date +%s)"
  elapsed=$((now - start_epoch))
  capture_sample "heating" "${elapsed}" | tee -a "${SAMPLES_FILE}" >/dev/null

  latest_line="$(tail -n 1 "${SAMPLES_FILE}")"
  reliable="$(printf '%s\n' "${latest_line}" | jq -r '.spa.temperature_reliable')"
  temp="$(printf '%s\n' "${latest_line}" | jq -r '.spa.temperature')"

  if [[ "${reliable}" == "true" && "${temp}" != "null" && "${temp}" -ge "${TARGET_TEMP}" ]]; then
    reached=1
    break
  fi

  if [[ "${now}" -ge "${deadline}" ]]; then
    break
  fi

  sleep "${SAMPLE_SECONDS}"
done

cleanup
trap - EXIT
capture_sample "restored" "$(( $(date +%s) - start_epoch ))" >>"${SAMPLES_FILE}"

jq -n \
  --arg base_url "${BASE_URL}" \
  --arg samples_file "${SAMPLES_FILE}" \
  --arg started_at "$(head -n 1 "${SAMPLES_FILE}" | jq -r '.captured_at')" \
  --arg ended_at "$(tail -n 1 "${SAMPLES_FILE}" | jq -r '.captured_at')" \
  --argjson target_temperature "${TARGET_TEMP}" \
  --argjson reached_target "${reached}" \
  --argjson sample_seconds "${SAMPLE_SECONDS}" \
  --argjson timeout_seconds "${TIMEOUT_SECONDS}" \
  '{
      base_url: $base_url,
      samples_file: $samples_file,
      target_temperature: $target_temperature,
      reached_target: ($reached_target == 1),
      sample_seconds: $sample_seconds,
      timeout_seconds: $timeout_seconds,
      started_at: $started_at,
      ended_at: $ended_at
    }' >"${SUMMARY_FILE}"

echo "${SUMMARY_FILE}"
