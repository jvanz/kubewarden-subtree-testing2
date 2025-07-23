package controller

import (
	"context"
	"errors"

	k8spoliciesv1 "k8s.io/api/policy/v1"
	metav1 "k8s.io/apimachinery/pkg/apis/meta/v1"
	"sigs.k8s.io/controller-runtime/pkg/client"
	"sigs.k8s.io/controller-runtime/pkg/controller/controllerutil"

	policiesv1 "github.com/kubewarden/kubewarden-controller/api/policies/v1"
	"github.com/kubewarden/kubewarden-controller/internal/constants"
)

func (r *PolicyServerReconciler) reconcilePolicyServerPodDisruptionBudget(ctx context.Context, policyServer *policiesv1.PolicyServer) error {
	if policyServer.Spec.MinAvailable != nil || policyServer.Spec.MaxUnavailable != nil {
		return reconcilePodDisruptionBudget(ctx, policyServer, r.Client, r.DeploymentsNamespace)
	}
	return deletePodDisruptionBudget(ctx, policyServer, r.Client, r.DeploymentsNamespace)
}

func deletePodDisruptionBudget(ctx context.Context, policyServer *policiesv1.PolicyServer, k8s client.Client, namespace string) error {
	pdb := &k8spoliciesv1.PodDisruptionBudget{
		ObjectMeta: metav1.ObjectMeta{
			Name:      policyServer.NameWithPrefix(),
			Namespace: namespace,
		},
	}

	err := client.IgnoreNotFound(k8s.Delete(ctx, pdb))
	if err != nil {
		err = errors.Join(errors.New("failed to delete PodDisruptionBudget"), err)
	}

	return err
}

func reconcilePodDisruptionBudget(ctx context.Context, policyServer *policiesv1.PolicyServer, k8s client.Client, namespace string) error {
	commonLabels := policyServer.CommonLabels()
	pdb := &k8spoliciesv1.PodDisruptionBudget{
		ObjectMeta: metav1.ObjectMeta{
			Name:      policyServer.NameWithPrefix(),
			Namespace: namespace,
			Labels:    commonLabels,
		},
	}
	_, err := controllerutil.CreateOrPatch(ctx, k8s, pdb, func() error {
		pdb.Name = policyServer.NameWithPrefix()
		pdb.Namespace = namespace
		if err := controllerutil.SetOwnerReference(policyServer, pdb, k8s.Scheme()); err != nil {
			return errors.Join(errors.New("failed to set policy server PDB owner reference"), err)
		}

		pdb.Spec.Selector = &metav1.LabelSelector{
			MatchLabels: map[string]string{
				constants.InstanceLabelKey:     commonLabels[constants.InstanceLabelKey],
				constants.PartOfLabelKey:       commonLabels[constants.PartOfLabelKey],
				constants.PolicyServerLabelKey: policyServer.GetName(),
			},
		}
		if policyServer.Spec.MinAvailable != nil {
			pdb.Spec.MinAvailable = policyServer.Spec.MinAvailable
			pdb.Spec.MaxUnavailable = nil
		} else {
			pdb.Spec.MaxUnavailable = policyServer.Spec.MaxUnavailable
			pdb.Spec.MinAvailable = nil
		}
		return nil
	})
	if err != nil {
		err = errors.Join(errors.New("failed to create or update PodDisruptionBudget"), err)
	}

	return err
}
