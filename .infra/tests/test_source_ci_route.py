import pathlib
import tomllib
import unittest


ROOT = pathlib.Path(__file__).resolve().parents[2]


class SourceCiRouteTests(unittest.TestCase):
    def test_ordinary_tasks_exclude_archive_but_keep_other_gates(self):
        with (ROOT / ".infra/ci/ci.toml").open("rb") as config_file:
            tasks = tomllib.load(config_file)["tasks"]

        self.assertNotIn("package", tasks)
        self.assertNotIn("verify", tasks)
        for task in [
            "style",
            "clippy",
            "strict-doc",
            "readme",
            "feature-matrix",
            "fuzz",
            "project-hook",
            "coverage",
            "audit",
        ]:
            self.assertIn(task, tasks)

    def test_verify_route_runs_only_shared_source_suites(self):
        check = (ROOT / ".infra/bin/ci-check.sh").read_text()
        bootstrap = (ROOT / ".infra/bootstrap.sh").read_text()
        for suite in ["lock", "build", "test", "doc"]:
            self.assertIn(suite, bootstrap)
        self.assertIn("--only=verify", check)
        self.assertIn("source-verify", check)
        self.assertIn("RS_INFRA_CI_ROUTE", bootstrap)
        self.assertNotIn('run --suite package', bootstrap)
        self.assertIn("source-full", bootstrap)
        self.assertIn("source-full-plan", bootstrap)

    def test_reviewable_plan_matches_wrapper_and_hosted_route(self):
        plan = (ROOT / ".infra/ci/source-verification-plan.md").read_text()
        for expected in [
            "./.infra/bin/ci-check.sh",
            "./.infra/bin/ci-check.sh --only=verify",
            "rs-infra-verify run --suite lock",
            "rs-infra-verify run --suite build",
            "rs-infra-verify run --suite test",
            "rs-infra-verify run --suite doc",
            "feature matrix",
            "coverage",
        ]:
            self.assertIn(expected, plan)

    def test_documentation_profiles_select_sources_and_lock_the_resolved_graph(self):
        harness = (ROOT / "tests/documentation_examples_tests.rs").read_text()
        selection = harness.split("fn test_readme_and_user_guide_examples_compile()", 1)[1]
        self.assertIn('candidate_source_root("QUBIT_IO_CANDIDATE_MANIFEST")', selection)
        self.assertIn('candidate_source_root("QUBIT_SPI_CANDIDATE_MANIFEST")', selection)
        self.assertIn('.arg("generate-lockfile")', selection)
        self.assertIn('args(["metadata", "--locked", "--format-version", "1"])', selection)
        self.assertIn("pin_profile_package(root, workspace.path(), \"qubit-io\"", selection)
        self.assertIn("validate_candidate_local_files_edge(&graph", selection)
        self.assertIn("candidate_sha(&path)", selection)
        self.assertIn("identities=[{identities}]", selection)
        source_selection = selection.split("let mut candidate_sources", 1)[1].split("let documents", 1)[0]
        self.assertNotIn("is_file()", source_selection)
        self.assertIn('"qubit-fs-registry"', source_selection)
        self.assertIn('"qubit-local-files"', source_selection)

    def test_hosted_workflow_keeps_shared_runner_and_windows_suite(self):
        workflow = (ROOT / ".github/workflows/ci.yml").read_text()
        self.assertIn("source-route-contract:", workflow)
        self.assertIn("uses: qubit-ltd/rs-infra-ci/.github/workflows/github-ci.yml@main", workflow)
        self.assertIn("cargo test --locked --no-default-features --features registry --lib --tests", workflow)
        self.assertNotIn("run --suite package", workflow)
        release = (ROOT / ".github/workflows/package-archive.yml").read_text()
        self.assertIn("workflow_dispatch:", release)
        self.assertIn("run --suite package", release)


if __name__ == "__main__":
    unittest.main()
