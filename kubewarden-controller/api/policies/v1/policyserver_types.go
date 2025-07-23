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
	"github.com/kubewarden/kubewarden-controller/internal/constants"
	corev1 "k8s.io/api/core/v1"
	metav1 "k8s.io/apimachinery/pkg/apis/meta/v1"
	"k8s.io/apimachinery/pkg/util/intstr"
)

// PolicyServerSecurity defines securityContext configuration to be used in the Policy Server workload.
type PolicyServerSecurity struct {
	// securityContext definition to be used in the policy server container
	// +optional
	Container *corev1.SecurityContext `json:"container,omitempty"`
	// podSecurityContext definition to be used in the policy server Pod
	// +optional
	Pod *corev1.PodSecurityContext `json:"pod,omitempty"`
}

// PolicyServerSpec defines the desired state of PolicyServer.
type PolicyServerSpec struct {
	// Docker image name.
	Image string `json:"image"`

	// Replicas is the number of desired replicas.
	Replicas int32 `json:"replicas"`

	// Number of policy server replicas that must be still available after the
	// eviction. The value can be an absolute number or a percentage. Only one of
	// MinAvailable or Max MaxUnavailable can be set.
	MinAvailable *intstr.IntOrString `json:"minAvailable,omitempty"`

	// Number of policy server replicas that can be unavailable after the
	// eviction. The value can be an absolute number or a percentage. Only one of
	// MinAvailable or Max MaxUnavailable can be set.
	MaxUnavailable *intstr.IntOrString `json:"maxUnavailable,omitempty"`

	// Annotations is an unstructured key value map stored with a resource that may be
	// set by external tools to store and retrieve arbitrary metadata. They are not
	// queryable and should be preserved when modifying objects.
	// More info: https://kubernetes.io/docs/concepts/overview/working-with-objects/annotations/
	// +optional
	Annotations map[string]string `json:"annotations,omitempty"`

	// List of environment variables to set in the container.
	// +optional
	Env []corev1.EnvVar `json:"env,omitempty"`

	// Name of the service account associated with the policy server.
	// Namespace service account will be used if not specified.
	// +optional
	ServiceAccountName string `json:"serviceAccountName,omitempty"`

	// Name of ImagePullSecret secret in the same namespace, used for pulling
	// policies from repositories.
	// +optional
	ImagePullSecret string `json:"imagePullSecret,omitempty"`

	// List of insecure URIs to policy repositories. The `insecureSources`
	// content format corresponds with the contents of the `insecure_sources`
	// key in `sources.yaml`. Reference for `sources.yaml` is found in the
	// Kubewarden documentation in the reference section.
	// +optional
	InsecureSources []string `json:"insecureSources,omitempty"`

	// Key value map of registry URIs endpoints to a list of their associated
	// PEM encoded certificate authorities that have to be used to verify the
	// certificate used by the endpoint. The `sourceAuthorities` content format
	// corresponds with the contents of the `source_authorities` key in
	// `sources.yaml`. Reference for `sources.yaml` is found in the Kubewarden
	// documentation in the reference section.
	// +optional
	SourceAuthorities map[string][]string `json:"sourceAuthorities,omitempty"`

	// Name of VerificationConfig configmap in the same namespace, containing
	// Sigstore verification configuration. The configuration must be under a
	// key named verification-config in the Configmap.
	// +optional
	VerificationConfig string `json:"verificationConfig,omitempty"`

	// Security configuration to be used in the Policy Server workload.
	// The field allows different configurations for the pod and containers.
	// If set for the containers, this configuration will not be used in
	// containers added by other controllers (e.g. telemetry sidecars)
	// +optional
	SecurityContexts PolicyServerSecurity `json:"securityContexts,omitempty"`

	// Affinity rules for the associated Policy Server pods.
	// +optional
	Affinity corev1.Affinity `json:"affinity,omitempty"`

	// Limits describes the maximum amount of compute resources allowed.
	// +optional
	Limits corev1.ResourceList `json:"limits,omitempty"`

	// Requests describes the minimum amount of compute resources required.
	// If Request is omitted for, it defaults to Limits if that is explicitly specified,
	// otherwise to an implementation-defined value
	// +optional
	Requests corev1.ResourceList `json:"requests,omitempty"`

	// Tolerations describe the policy server pod's tolerations. It can be
	// used to ensure that the policy server pod is not scheduled onto a
	// node with a taint.
	Tolerations []corev1.Toleration `json:"tolerations,omitempty"`

	// PriorityClassName is the name of the PriorityClass to be used for the
	// policy server pods. Useful to schedule policy server pods with higher
	// priority to ensure their availability over other cluster workload
	// resources.
	// Note: If the referenced PriorityClass is deleted, existing pods
	// remain unchanged, but new pods that reference it cannot be created.
	// +optional
	PriorityClassName string `json:"priorityClassName,omitempty"`
}

type ReconciliationTransitionReason string

const (
	// ReconciliationFailed represents a reconciliation failure.
	ReconciliationFailed ReconciliationTransitionReason = "ReconciliationFailed"
	// ReconciliationSucceeded represents a reconciliation success.
	ReconciliationSucceeded ReconciliationTransitionReason = "ReconciliationSucceeded"
)

type PolicyServerConditionType string

const (
	// PolicyServerCertSecretReconciled represents the condition of the
	// Policy Server Secret reconciliation.
	PolicyServerCertSecretReconciled PolicyServerConditionType = "CertSecretReconciled"
	// CARootSecretReconciled represents the condition of the
	// Policy Server CA Root Secret reconciliation.
	CARootSecretReconciled PolicyServerConditionType = "CARootSecretReconciled"
	// PolicyServerConfigMapReconciled represents the condition of the
	// Policy Server ConfigMap reconciliation.
	PolicyServerConfigMapReconciled PolicyServerConditionType = "ConfigMapReconciled"
	// PolicyServerDeploymentReconciled represents the condition of the
	// Policy Server Deployment reconciliation.
	PolicyServerDeploymentReconciled PolicyServerConditionType = "DeploymentReconciled"
	// PolicyServerServiceReconciled represents the condition of the
	// Policy Server Service reconciliation.
	PolicyServerServiceReconciled PolicyServerConditionType = "ServiceReconciled"
	// PolicyServerPodDisruptionBudgetReconciled represents the condition of the
	// Policy Server PodDisruptionBudget reconciliation.
	PolicyServerPodDisruptionBudgetReconciled PolicyServerConditionType = "PodDisruptionBudgetReconciled"
)

// PolicyServerStatus defines the observed state of PolicyServer.
type PolicyServerStatus struct {
	// Conditions represent the observed conditions of the
	// PolicyServer resource.  Known .status.conditions.types
	// are: "PolicyServerSecretReconciled",
	// "PolicyServerDeploymentReconciled" and
	// "PolicyServerServiceReconciled"
	// +patchMergeKey=type
	// +patchStrategy=merge
	// +listType=map
	// +listMapKey=type
	Conditions []metav1.Condition `json:"conditions"`
}

//+kubebuilder:object:root=true
//+kubebuilder:subresource:status
//+kubebuilder:resource:scope=Cluster,shortName=ps
//+kubebuilder:printcolumn:name="Replicas",type=string,JSONPath=`.spec.replicas`,description="Policy Server replicas"
//+kubebuilder:printcolumn:name="Image",type=string,JSONPath=`.spec.image`,description="Policy Server image"
//+kubebuilder:storageversion

// PolicyServer is the Schema for the policyservers API.
type PolicyServer struct {
	metav1.TypeMeta   `json:",inline"`
	metav1.ObjectMeta `json:"metadata,omitempty"`

	Spec   PolicyServerSpec   `json:"spec,omitempty"`
	Status PolicyServerStatus `json:"status,omitempty"`
}

func (ps *PolicyServer) NameWithPrefix() string {
	return "policy-server-" + ps.Name
}

func (ps *PolicyServer) AppLabel() string {
	return "kubewarden-" + ps.NameWithPrefix()
}

// CommonLabels returns the common labels to be used with the resources
// associated to a Policy Server. The labels defined follow
// Kubernetes guidelines: https://kubernetes.io/docs/concepts/overview/working-with-objects/common-labels/#labels
func (ps *PolicyServer) CommonLabels() map[string]string {
	return map[string]string{
		constants.ComponentLabelKey: constants.ComponentPolicyServerLabelValue,
		constants.InstanceLabelKey:  ps.NameWithPrefix(),
		constants.PartOfLabelKey:    constants.PartOfLabelValue,
		constants.ManagedByKey:      "kubewarden-controller",
	}
}

//+kubebuilder:object:root=true

// PolicyServerList contains a list of PolicyServer.
type PolicyServerList struct {
	metav1.TypeMeta `json:",inline"`
	metav1.ListMeta `json:"metadata,omitempty"`
	Items           []PolicyServer `json:"items"`
}

func init() {
	SchemeBuilder.Register(&PolicyServer{}, &PolicyServerList{})
}
