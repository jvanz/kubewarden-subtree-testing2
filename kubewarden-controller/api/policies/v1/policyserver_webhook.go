/*


Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

    http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.
*/

package v1

import (
	"context"
	"fmt"

	corev1 "k8s.io/api/core/v1"
	apierrors "k8s.io/apimachinery/pkg/api/errors"
	"k8s.io/apimachinery/pkg/api/resource"
	"k8s.io/apimachinery/pkg/api/validation"
	"k8s.io/apimachinery/pkg/runtime"
	validationutils "k8s.io/apimachinery/pkg/util/validation"
	"k8s.io/apimachinery/pkg/util/validation/field"
	ctrl "sigs.k8s.io/controller-runtime"
	"sigs.k8s.io/controller-runtime/pkg/client"
	"sigs.k8s.io/controller-runtime/pkg/controller/controllerutil"
	"sigs.k8s.io/controller-runtime/pkg/webhook"
	"sigs.k8s.io/controller-runtime/pkg/webhook/admission"

	"github.com/go-logr/logr"
	"github.com/kubewarden/kubewarden-controller/internal/constants"
)

// SetupWebhookWithManager registers the PolicyServer webhook with the controller manager.
func (ps *PolicyServer) SetupWebhookWithManager(mgr ctrl.Manager, deploymentsNamespace string) error {
	logger := mgr.GetLogger().WithName("policyserver-webhook")

	err := ctrl.NewWebhookManagedBy(mgr).
		For(ps).
		WithDefaulter(&policyServerDefaulter{
			logger: logger,
		}).
		WithValidator(&policyServerValidator{
			deploymentsNamespace: deploymentsNamespace,
			k8sClient:            mgr.GetClient(),
			logger:               logger,
		}).
		Complete()
	if err != nil {
		return fmt.Errorf("failed enrolling webhook with manager: %w", err)
	}

	return nil
}

// +kubebuilder:webhook:path=/mutate-policies-kubewarden-io-v1-policyserver,mutating=true,failurePolicy=fail,sideEffects=None,groups=policies.kubewarden.io,resources=policyservers,verbs=create;update,versions=v1,name=mpolicyserver.kb.io,admissionReviewVersions={v1,v1beta1}

// policyServerDefaulter sets defaults of PolicyServer objects when they are created or updated.
type policyServerDefaulter struct {
	logger logr.Logger
}

var _ webhook.CustomDefaulter = &policyServerDefaulter{}

// Default implements webhook.CustomDefaulter so a webhook will be registered for the type.
func (d *policyServerDefaulter) Default(_ context.Context, obj runtime.Object) error {
	policyServer, ok := obj.(*PolicyServer)
	if !ok {
		return fmt.Errorf("expected a PolicyServer object, got %T", obj)
	}

	d.logger.Info("Defaulting PolicyServer", "name", policyServer.GetName())

	if policyServer.ObjectMeta.DeletionTimestamp == nil {
		controllerutil.AddFinalizer(policyServer, constants.KubewardenFinalizer)
	}

	return nil
}

// +kubebuilder:webhook:path=/validate-policies-kubewarden-io-v1-policyserver,mutating=false,failurePolicy=fail,sideEffects=None,groups=policies.kubewarden.io,resources=policyservers,verbs=create;update,versions=v1,name=vpolicyserver.kb.io,admissionReviewVersions=v1

// polyServerCustomValidator validates PolicyServers when they are created, updated, or deleted.
type policyServerValidator struct {
	deploymentsNamespace string
	k8sClient            client.Client
	logger               logr.Logger
}

var _ webhook.CustomValidator = &policyServerValidator{}

// ValidateCreate implements webhook.CustomValidator so a webhook will be registered for the type.
func (v *policyServerValidator) ValidateCreate(ctx context.Context, obj runtime.Object) (admission.Warnings, error) {
	policyServer, ok := obj.(*PolicyServer)
	if !ok {
		return nil, fmt.Errorf("expected a PolicyServer object, got %T", obj)
	}

	v.logger.Info("Validating PolicyServer create", "name", policyServer.GetName())

	return nil, v.validate(ctx, policyServer)
}

// ValidateUpdate implements webhook.CustomValidator so a webhook will be registered for the type.)
func (v *policyServerValidator) ValidateUpdate(ctx context.Context, _, newObj runtime.Object) (admission.Warnings, error) {
	policyServer, ok := newObj.(*PolicyServer)
	if !ok {
		return nil, fmt.Errorf("expected a PolicyServer object, got %T", newObj)
	}

	v.logger.Info("Validating PolicyServer update", "name", policyServer.GetName())

	return nil, v.validate(ctx, policyServer)
}

// ValdidaeDelete implements webhook.CustomValidator so a webhook will be registered for the type.
func (v *policyServerValidator) ValidateDelete(_ context.Context, obj runtime.Object) (admission.Warnings, error) {
	policyServer, ok := obj.(*PolicyServer)
	if !ok {
		return nil, fmt.Errorf("expected a PolicyServer object, got %T", obj)
	}

	v.logger.Info("Validating PolicyServer delete", "name", policyServer.GetName())

	return nil, nil
}

// validate validates a the fields PolicyServer object.
func (v *policyServerValidator) validate(ctx context.Context, policyServer *PolicyServer) error {
	var allErrs field.ErrorList

	// The PolicyServer name must be maximum 63 like all Kubernetes objects to fit in a DNS subdomain name
	if len(policyServer.GetName()) > validationutils.DNS1035LabelMaxLength {
		allErrs = append(allErrs, field.Invalid(field.NewPath("metadata").Child("name"), policyServer.GetName(), fmt.Sprintf("the PolicyServer name cannot be longer than %d characters", validationutils.DNS1035LabelMaxLength)))
	}

	if policyServer.Spec.ImagePullSecret != "" {
		if err := validateImagePullSecret(ctx, v.k8sClient, policyServer.Spec.ImagePullSecret, v.deploymentsNamespace); err != nil {
			allErrs = append(allErrs, field.Invalid(field.NewPath("spec").Child("imagePullSecret"), policyServer.Spec.ImagePullSecret, err.Error()))
		}
	}

	// Kubernetes does not allow to set both MinAvailable and MaxUnavailable at the same time
	if policyServer.Spec.MinAvailable != nil && policyServer.Spec.MaxUnavailable != nil {
		allErrs = append(allErrs, field.Invalid(field.NewPath("spec"), fmt.Sprintf("minAvailable: %s, maxUnavailable: %s", policyServer.Spec.MinAvailable, policyServer.Spec.MaxUnavailable), "minAvailable and maxUnavailable cannot be both set"))
	}

	allErrs = append(allErrs, validateLimitsAndRequests(policyServer.Spec.Limits, policyServer.Spec.Requests)...)

	if len(allErrs) == 0 {
		return nil
	}

	return apierrors.NewInvalid(GroupVersion.WithKind("PolicyServer").GroupKind(), policyServer.Name, allErrs)
}

// validateImagePullSecret validates that the specified PolicyServer imagePullSecret exists and is of type kubernetes.io/dockerconfigjson.
func validateImagePullSecret(ctx context.Context, k8sClient client.Client, imagePullSecret string, deploymentsNamespace string) error {
	secret := &corev1.Secret{}
	err := k8sClient.Get(ctx, client.ObjectKey{
		Namespace: deploymentsNamespace,
		Name:      imagePullSecret,
	}, secret)
	if err != nil {
		return fmt.Errorf("cannot get spec.ImagePullSecret: %w", err)
	}

	if secret.Type != "kubernetes.io/dockerconfigjson" {
		return fmt.Errorf("spec.ImagePullSecret secret \"%s\" is not of type kubernetes.io/dockerconfigjson", secret.Name)
	}

	return nil
}

// validateLimitsAndRequests validates that the specified PolicyServer limits and requests are not negative and requests are less than or equal to limits.
func validateLimitsAndRequests(limits, requests corev1.ResourceList) field.ErrorList {
	var allErrs field.ErrorList

	limitFieldPath := field.NewPath("spec").Child("limits")
	requestFieldPath := field.NewPath("spec").Child("requests")

	for limitName, limitQuantity := range limits {
		fieldPath := limitFieldPath.Child(string(limitName))
		if limitQuantity.Cmp(resource.Quantity{}) < 0 {
			allErrs = append(allErrs, field.Invalid(fieldPath, limitQuantity.String(), validation.IsNegativeErrorMsg))
		}
	}

	for requestName, requestQuantity := range requests {
		fieldPath := requestFieldPath.Child(string(requestName))
		if requestQuantity.Cmp(resource.Quantity{}) < 0 {
			allErrs = append(allErrs, field.Invalid(fieldPath, requestQuantity.String(), validation.IsNegativeErrorMsg))
		}

		limitQuantity, ok := limits[requestName]
		if !ok {
			continue
		}

		if requestQuantity.Cmp(limitQuantity) > 0 {
			allErrs = append(allErrs, field.Invalid(fieldPath, requestQuantity.String(), fmt.Sprintf("must be less than or equal to %s limit of %s", requestName, limitQuantity.String())))
		}
	}

	return allErrs
}
