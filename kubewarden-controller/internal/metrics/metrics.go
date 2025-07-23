package metrics

import (
	"context"
	"fmt"
	"time"

	"go.opentelemetry.io/otel"
	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/exporters/otlp/otlpmetric/otlpmetricgrpc"
	"go.opentelemetry.io/otel/metric"
	metricSDK "go.opentelemetry.io/otel/sdk/metric"

	policiesv1 "github.com/kubewarden/kubewarden-controller/api/policies/v1"
)

const (
	meterName                      = "kubewarden"
	policyCounterMetricName        = "kubewarden_policy_total"
	policyCounterMetricDescription = "How many policies are installed in the cluster"
	timeBetweenExports             = 2 * time.Second
)

func New() (func(context.Context) error, error) {
	ctx := context.Background()

	// Create the OTLP exporter to export metrics to the specified endpoint.
	// All the Otel exporter configuration is set by environment variables.
	exporter, err := otlpmetricgrpc.New(
		ctx,
	)
	if err != nil {
		return nil, fmt.Errorf("cannot start metric exporter: %w", err)
	}
	meterProvider := metricSDK.NewMeterProvider(metricSDK.WithReader(
		metricSDK.NewPeriodicReader(exporter, metricSDK.WithInterval(timeBetweenExports))))

	otel.SetMeterProvider(meterProvider)

	return meterProvider.Shutdown, nil
}

func RecordPolicyCount(ctx context.Context, policy policiesv1.Policy) error {
	failurePolicy := ""
	if policy.GetFailurePolicy() != nil {
		failurePolicy = string(*policy.GetFailurePolicy())
	}

	meter := otel.Meter(meterName)
	counter, err := meter.Int64Counter(policyCounterMetricName, metric.WithDescription(policyCounterMetricDescription))
	if err != nil {
		return fmt.Errorf("cannot create the instrument: %w", err)
	}

	commonLabels := []attribute.KeyValue{
		attribute.String("name", policy.GetUniqueName()),
		attribute.String("policy_server", policy.GetPolicyServer()),
		attribute.String("module", policy.GetModule()),
		attribute.Bool("mutating", policy.IsMutating()),
		attribute.String("namespace", policy.GetNamespace()),
		attribute.String("failure_policy", failurePolicy),
		attribute.String("policy_status", string(policy.GetStatus().PolicyStatus)),
	}
	counter.Add(ctx, 1, metric.WithAttributes(commonLabels...))

	return nil
}
