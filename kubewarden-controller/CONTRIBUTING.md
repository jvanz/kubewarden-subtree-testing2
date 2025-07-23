# Contributing

## Building

To build kubewarden-controller there are package requirements. If you are using
openSUSE Leap, you can install them with the following command:

```console
sudo zypper in -y make go
```

Then, can run the following command to build the package:

```console
make
```

## Development

To run the controller for development purposes, you can use [Tilt](https://tilt.dev/).

### Pre-requisites

Please follow the [Tilt installation
documentation](https://docs.tilt.dev/install.html) to install the CLI tool. You
need to clone the [kubewarden helm-charts
repository](https://github.com/kubewarden/helm-charts) in your local machine:

```console
git clone git@github.com/kubewarden/helm-charts.git
```

You need to clone the [kubewarden audit-scanner repository](https://github.com/kubewarden/audit-scanner) in your local machine:

```console
git clone git@github.com/kubewarden/audit-scanner.git
```

A development Kubernetes cluster is needed to run the controller.
You can use [k3d](https://k3d.io/) to create a local cluster for development purposes.

### Settings

The `tilt-settings.yaml.example` acts as a template for the `tilt-settings.yaml`
file that you need to create in the root of this repository. Copy the example
file and edit it to match your environment. The `tilt-settings.yaml` file is
ignored by git, so you can edit it without concern about committing it by
mistake.

The following settings can be configured:

- `registry`: the container registry to push the controller image to. If you
  don't have a private registry, you can use `ghcr.io` provided your cluster has
  access to it.

- `image`: the name of the controller image. If you are using `ghcr.io` as your
  registry, you need to prefix the image name with your GitHub username.

- `helm_charts_path`: the path to the `helm-charts` repository that you cloned
  in the previous step.

Example:

```yaml
registry: ghcr.io
image: your-github-username/kubewarden-controller
helmChartPath: /path/to/helm-charts
```

### Running the controller

The `Tiltfile` included in this repository takes care of the following:

- Install the CRDs from the `config/crd/` directory of this repository.
- Install `cert-manager`.
- Create the `kubewarden` namespace and install the controller helm-chart in it.
- Inject the development image in the deployment.
- Automatically reload the controller when you make changes to the code.

To run the controller, you just need to run the following command against an
empty cluster:

```console
tilt up --stream
```

## Testing

### Run tests

Run all tests:

```console
make test
```

Run unit tests only:

```console
make unit-tests
```

Run controller integration tests only:

```console
make integration-tests
```

Run tests that do not require a Kubernetes cluster only:

```console
make integration-tests-envtest
```

Run tests that require a Kubernetes cluster only:

```console
make integration-tests-real-cluster
```

### Writing (controller) integration tests

The controller integration tests are written using the [Ginkgo](https://onsi.github.io/ginkgo/)
and [Gomega](https://onsi.github.io/gomega/) testing frameworks.
The tests are located in the `internal/controller` package.

By default, the tests are run by using [envtest](https://book.kubebuilder.io/reference/envtest) which
sets up an instance of etcd and the Kubernetes API server, without kubelet, controller-manager or other components.

However, some tests require a real Kubernetes cluster to run.
These tests must be marked with the `real-cluster` ginkgo label.

Example:

```
var _ = Describe("A test that requires a real cluster", Label("real cluster") func() {
    Context("when running in a real cluster", func() {
        It("should do something", func() {
            // test code
        })
    })
})
```

To run only the tests that require a real cluster, you can use the following command:

```console
make integration-test-real-cluster
```

The suite setup will start a [k3s testcontainer](https://testcontainers.com/modules/k3s/) and run the tests against it.
It will also stop and remove the container when the tests finish.

Note that the `real-cluster` tests are slower than the `envtest` tests, therefore, it is recommended to keep the number of `real-cluster` tests to a minimum.
An example of a test that requires a real cluster is the `AdmissionPolicy` test suite, since at the time of writing, we wait for the `PolicyServer` Pod to be ready before reconciling the webhook configuration.

### Focusing

You can focus on a specific test or spec by using a [Focused Spec](https://onsi.github.io/ginkgo/#focused-specs).

Example:

```go
var _ = Describe("Controller test", func() {
    FIt("should do something", func() {
        // This spec will be the only one executed
    })
})
```

## Tagging a new release

### Make sure to update the CRD docs

```console
cd docs/crds
make generate
```

Commit the resulting changes.

### Create a new tag

Assuming your official Kubewarden remote is named `upstream`:

```console
git tag -a vX.Y.Z  -m "vX.Y.Z" -s
git push upstream main vX.Y.Z
```

Check that the GitHub actions run without
errors. Regarding the release, several automation tasks should
have been started:

1. Execute tests
1. Create a new GitHub release
1. Push a tagged container image with the build of the project

For a release to be complete, all these tasks should
run successfully.

### Consider bumping the helm-chart

Now that the controller has a new tag released, the automation bumps the
[Kubewarden
`helm-chart`](https://github.com/kubewarden/helm-charts/tree/main/charts/kubewarden-controller).

### Consider announcing the new release in channels!

## Kubewarden release template

If you are releasing the Kubewarden stack then follow these steps to ensure that
everything works well:

- [ ] Update controller code
- [ ] Run controller tests or check if the CI is green in the main branch
- [ ] Update audit scanner code
- [ ] Run audit scanner tests or check if the CI is green in the main branch
- [ ] Bump policy server version in the `Cargo.toml` and update the `Cargo.lock`
      file. This requires a PR in the repository to update the files in the main
      branch. Update the local code after merging the PR
- [ ] Run policy server tests or check if the CI is green in the main branch
- [ ] Bump kwctl version in the `Cargo.toml` and update the `Cargo.lock` file.
      This requires a PR in the repository to update the files in the main branch.
      Update the local code after merging the PR
- [ ] Run kwctl tests or check if the CI is green in the main branch
- [ ] Tag audit scanner
- [ ] Tag policy server
- [ ] Tag controller
- [ ] Tag kwctl
- [ ] Wait for all CI running in all the major components (audit scanner,
      controller, policy server and kwctl) to finish
- [ ] Check if the Helm chart repository CI open a PR updating the Helm charts
      with the correct changes.
  - [ ] Check if the `kubewarden-controller` chart versions are correctly bumped
  - [ ] Check if the `kubewarden-defaults` chart versions are correctly bumped
  - [ ] Check if the `kubewarden-crds` chart versions are correctly bumped
  - [ ] Check if kubewarden-controller, kubewarden-defaults and kubewarden-crds
        charts have the same `appVersion`
- [ ] Check if CI in the Helm chart PR is green. If so, merge it.
