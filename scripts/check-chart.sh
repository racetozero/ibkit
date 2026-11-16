#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
temporary=$(mktemp -d)
trap 'rm -rf "$temporary"' EXIT
helm lint charts/ibkit-gateway
helm template smoke charts/ibkit-gateway > "$temporary/default.yaml"
helm template smoke charts/ibkit-gateway --set persistence.enabled=false --set gateway.tradingMode=live --set gateway.apiPort=4001 > "$temporary/live.yaml"
helm template smoke charts/ibkit-gateway --set persistence.existingClaim=existing-state > "$temporary/claim.yaml"
helm template smoke charts/ibkit-gateway --set credentials.totpSecretKey=totp --set gateway.javaHeapMb=1536 --set gateway.masterClientId=0 --set gateway.passkeyAuthenticate=true > "$temporary/options.yaml"
grep -q 'path: totp' "$temporary/options.yaml"
grep -q 'master_client_id = 0' "$temporary/options.yaml"
helm template smoke charts/ibkit-gateway --set controlApi.enabled=true --set controlApi.existingSecret=ibkit-control > "$temporary/control.yaml"
grep -q 'control_listen = "0.0.0.0:5800"' "$temporary/control.yaml"
grep -q 'name: control' "$temporary/control.yaml"
helm template smoke charts/ibkit-gateway --set controlApi.enabled=true --set controlApi.existingSecret=ibkit-control --set controlApi.serviceMonitor.enabled=true --set alerts.existingSecret=ibkit-alerts > "$temporary/monitor.yaml"
grep -q 'kind: ServiceMonitor' "$temporary/monitor.yaml"
grep -q 'name: IBKIT_ALERT_WEBHOOK_URL' "$temporary/monitor.yaml"
helm template smoke charts/ibkit-gateway --set vnc.enabled=true --set vnc.existingSecret=ibkit-vnc --set gateway.apiAllowedSources=10.0.0.0/8 --set gateway.closedownAt='Fri 23:00' --set gateway.coldRestartAt='Sun 07:05' > "$temporary/features.yaml"
grep -q 'vnc_listen = "127.0.0.1:5900"' "$temporary/features.yaml"
grep -q 'path: password' "$temporary/features.yaml"
grep -q 'cold_restart_at = "Sun 07:05"' "$temporary/features.yaml"
if helm template smoke charts/ibkit-gateway --set vnc.enabled=true > /dev/null 2>&1; then
  echo 'The chart rendered VNC without a password Secret.' >&2
  exit 1
fi
if helm template smoke charts/ibkit-gateway --set controlApi.serviceMonitor.enabled=true > /dev/null 2>&1; then
  echo 'The chart rendered a ServiceMonitor without the control API.' >&2
  exit 1
fi
if helm template smoke charts/ibkit-gateway --set controlApi.enabled=true > /dev/null 2>&1; then
  echo 'The chart rendered the control API without a token Secret.' >&2
  exit 1
fi
# Feed each generated TOML document to the actual command parser. This catches
# quoting, field-name, and type faults that Helm's YAML parser cannot detect.
uv run --with pyyaml python - "$temporary" <<'PY'
import subprocess
import sys
from pathlib import Path
import yaml

root = Path(sys.argv[1])
for name in ('default', 'live', 'claim', 'options', 'control', 'monitor', 'features'):
    docs = list(yaml.safe_load_all((root / f'{name}.yaml').read_text()))
    configmap = next(doc for doc in docs if doc and doc['kind'] == 'ConfigMap')
    config = root / f'{name}.toml'
    config.write_text(configmap['data']['config.toml'])
    subprocess.run(['cargo', 'run', '-q', '-p', 'ibkit-cli', '--', 'gateway', '--config', str(config), 'validate'], check=True)
print('All rendered chart configurations are valid.')
PY
