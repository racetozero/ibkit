{{- define "ibkit.name" -}}
{{- printf "%s-ibkit" .Release.Name | trunc 63 | trimSuffix "-" -}}
{{- end -}}
{{- define "ibkit.selector" -}}
app.kubernetes.io/name: ibkit
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end -}}
{{- define "ibkit.labels" -}}
{{ include "ibkit.selector" . }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
helm.sh/chart: {{ printf "%s-%s" .Chart.Name .Chart.Version | quote }}
{{- end -}}
