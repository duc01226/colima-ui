//! Real application library boundaries, run once per supported provider through
//! `npm run qualify:windows`. This is not installed GUI/IPC qualification.
//! Missing prerequisites fail the run; the runner refuses non-Windows 11/x64.

mod native {
    use crate::commands::{
        compose, containers, file_transfer, metrics_collector, networks, volumes,
    };
    use crate::terminal_session::{create_shared_session, SessionKind, SessionManager};
    use crate::transfer_registry::{self, TransferStatus};
    use crate::windows_test_fixture::Fixture;
    use sha2::{Digest, Sha256};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    fn sha256(bytes: &[u8]) -> String {
        Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    struct AmbientSelectors(Vec<(&'static str, Option<std::ffi::OsString>)>);
    impl AmbientSelectors {
        fn hostile() -> Self {
            let old = ["DOCKER_CONTEXT", "DOCKER_HOST"]
                .into_iter()
                .map(|key| (key, std::env::var_os(key)))
                .collect();
            std::env::set_var("DOCKER_CONTEXT", "colimaui-does-not-exist");
            std::env::set_var("DOCKER_HOST", "tcp://127.0.0.1:1");
            Self(old)
        }
    }
    impl Drop for AmbientSelectors {
        fn drop(&mut self) {
            for (key, value) in &self.0 {
                match value {
                    Some(value) => std::env::set_var(key, value),
                    None => std::env::remove_var(key),
                }
            }
        }
    }

    async fn finished(job_id: &str) {
        let deadline = Instant::now() + Duration::from_secs(180);
        loop {
            let job = transfer_registry::list()
                .into_iter()
                .find(|job| job.job_id == job_id)
                .expect("registered transfer remains observable");
            if job.status.is_terminal() {
                assert_eq!(
                    job.status,
                    TransferStatus::Success,
                    "transfer failed: {:?}",
                    job.error
                );
                return;
            }
            assert!(Instant::now() < deadline, "transfer never settled");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    fn terminal_output(output: &Arc<Mutex<String>>, marker: &str) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !output.lock().unwrap().contains(marker) {
            assert!(
                Instant::now() < deadline,
                "terminal did not produce expected output"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn print_command(text: &str) -> String {
        // Expected output never appears literally in the input/terminal echo.
        let encoded: String = text
            .as_bytes()
            .iter()
            .map(|b| format!("\\{b:03o}"))
            .collect();
        format!("printf '{encoded}\\n'\n")
    }

    #[tokio::test(flavor = "multi_thread")]
    #[ignore = "native dedicated-engine suite; run via windows-qualify.mjs"]
    async fn native_provider_core_contract() {
        let mut fixture = Fixture::acquire().expect("required native fixture preflight");
        // Establish an external retained reference BEFORE adding/removing any
        // run-owned tag. Removing the final reference could delete the base.
        let base = fixture.inspect("image", &fixture.image).unwrap();
        let retained_tags: std::collections::BTreeSet<String> = base[0]["RepoTags"]
            .as_array()
            .expect("ENVIRONMENT-BLOCKED: fixture needs a retained pre-existing tag")
            .iter()
            .filter_map(|value| value.as_str())
            .filter(|tag| !tag.is_empty() && !tag.starts_with('<'))
            .map(str::to_owned)
            .collect();
        let retained_tag = retained_tags
            .iter()
            .next()
            .expect("ENVIRONMENT-BLOCKED: fixture needs a retained pre-existing tag")
            .clone();
        assert_eq!(
            fixture.inspect("image", &retained_tag).unwrap()[0]["Id"],
            fixture.image_id
        );
        let tag = fixture.name("image");
        fixture.claim("image", &tag);
        containers::tag_image(fixture.image_id.clone(), tag.clone())
            .await
            .unwrap();
        assert_eq!(
            fixture.inspect("image", &tag).unwrap()[0]["Id"].as_str(),
            Some(fixture.image_id.as_str())
        );

        let name = fixture.name("container");
        fixture.claim("container", &name);
        let id = containers::run_container(
            tag.clone(),
            name.clone(),
            vec![],
            vec![],
            vec![],
            true,
            false,
            vec![
                "--label".into(),
                format!("io.colimaui.test.run={}", fixture.run),
            ],
        )
        .await
        .unwrap();
        assert!(crate::validation::is_stable_container_id(&id));
        assert!(containers::list_containers_cli(true)
            .await
            .unwrap()
            .iter()
            .any(|row| row["ID"]
                .as_str()
                .is_some_and(|value| id.starts_with(value))));
        let inspected: serde_json::Value =
            serde_json::from_str(&containers::inspect_container(id.clone()).await.unwrap())
                .unwrap();
        assert_eq!(inspected[0]["State"]["Running"], true);
        assert_eq!(
            containers::container_exec(id.clone(), "printf visible-result".into())
                .await
                .unwrap(),
            "visible-result"
        );
        {
            let _ambient = AmbientSelectors::hostile();
            assert_eq!(
                containers::container_exec(id.clone(), "printf pinned-origin".into())
                    .await
                    .unwrap(),
                "pinned-origin"
            );
            assert_eq!(
                fixture.inspect("container", &id).unwrap()[0]["Id"].as_str(),
                Some(id.as_str())
            );
        }
        assert!(containers::container_logs(id.clone(), 20)
            .await
            .unwrap()
            .is_empty());
        containers::pause_container(id.clone()).await.unwrap();
        assert_eq!(
            fixture.inspect("container", &id).unwrap()[0]["State"]["Paused"],
            true
        );
        containers::unpause_container(id.clone()).await.unwrap();

        let volume = fixture.name("volume");
        fixture.claim("volume", &volume);
        volumes::create_volume(volume.clone(), "local".into())
            .await
            .unwrap();
        let volume_json: serde_json::Value =
            serde_json::from_str(&volumes::inspect_volume(volume.clone()).await.unwrap()).unwrap();
        assert_eq!(volume_json[0]["Name"].as_str(), Some(volume.as_str()));
        let network = fixture.name("network");
        fixture.claim("network", &network);
        networks::create_network(network.clone(), "bridge".into(), "".into())
            .await
            .unwrap();
        let network_json: serde_json::Value =
            serde_json::from_str(&networks::inspect_network(network.clone()).await.unwrap())
                .unwrap();
        assert_eq!(network_json[0]["Name"].as_str(), Some(network.as_str()));

        let payload = "literal data 空間\nwithout path reinterpretation\n";
        let source = fixture.work.join("payload 空間.txt");
        std::fs::write(&source, payload).unwrap();
        let copied = file_transfer::start_copy_to_container(
            None,
            id.clone(),
            source.to_str().unwrap().into(),
            "/tmp/payload.txt".into(),
        )
        .unwrap();
        finished(&copied.job_id).await;
        let expected_hash = sha256(payload.as_bytes());
        let hash = containers::container_exec(id.clone(), "sha256sum /tmp/payload.txt".into())
            .await
            .unwrap();
        assert_eq!(hash.split_whitespace().next(), Some(expected_hash.as_str()));
        let folder = fixture.work.to_str().unwrap().to_string();
        assert!(file_transfer::start_copy_from_container(
            None,
            id.clone(),
            "/tmp/payload.txt".into(),
            folder.clone(),
            "../escape.tar".into(),
            false
        )
        .is_err());
        let copied = file_transfer::start_copy_from_container(
            None,
            id.clone(),
            "/tmp/payload.txt".into(),
            folder.clone(),
            "copy 空間.tar".into(),
            false,
        )
        .unwrap();
        finished(&copied.job_id).await;
        let copied = file_transfer::start_copy_to_container(
            None,
            id.clone(),
            fixture.work.join("copy 空間.tar").to_str().unwrap().into(),
            "/tmp/roundtrip.tar".into(),
        )
        .unwrap();
        finished(&copied.job_id).await;
        assert_eq!(
            containers::container_exec(
                id.clone(),
                "tar -xOf /tmp/roundtrip.tar payload.txt".into()
            )
            .await
            .unwrap(),
            payload
        );
        let saved = file_transfer::start_image_save(
            None,
            vec![tag.clone()],
            folder,
            "image 空間.tar".into(),
            false,
        )
        .unwrap();
        finished(&saved.job_id).await;
        // TestSpec=TC-WINUI-012 — native roundtrip subset; installed/full case pending.
        // Import must restore something absent, rather than pass against the
        // tag already present before load. Only our unique tag is untagged.
        containers::remove_image(tag.clone(), false).await.unwrap();
        let absent = fixture.output(&["image", "inspect", "--", &tag]).unwrap();
        assert!(
            !absent.status.success(),
            "export tag must be absent before import"
        );
        assert!(String::from_utf8_lossy(&absent.stderr)
            .to_ascii_lowercase()
            .contains("no such image"));
        let preserved = fixture.inspect("image", &retained_tag).unwrap();
        assert_eq!(preserved[0]["Id"], fixture.image_id);
        let actual_tags: std::collections::BTreeSet<String> = preserved[0]["RepoTags"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap().to_owned())
            .collect();
        assert_eq!(
            actual_tags, retained_tags,
            "all pre-existing tags survive owned untag"
        );
        assert_eq!(
            fixture.inspect("container", &id).unwrap()[0]["State"]["Running"],
            true
        );
        let imported = file_transfer::start_image_load(
            None,
            fixture.work.join("image 空間.tar").to_str().unwrap().into(),
        )
        .unwrap();
        finished(&imported.job_id).await;
        assert_eq!(
            fixture.inspect("image", &tag).unwrap()[0]["Id"].as_str(),
            Some(fixture.image_id.as_str())
        );
        assert_eq!(
            fixture.inspect("image", &retained_tag).unwrap()[0]["Id"],
            fixture.image_id
        );
        assert_eq!(
            fixture.inspect("container", &id).unwrap()[0]["State"]["Running"],
            true
        );

        let output = Arc::new(Mutex::new(String::new()));
        let received = output.clone();
        let terminals = Arc::new(Mutex::new(SessionManager::new()));
        create_shared_session(
            terminals.clone(),
            "native-fixture".into(),
            SessionKind::ContainerExec {
                container_id: id.clone(),
                engine: fixture.context.clone(),
            },
            Box::new(move |text: String| received.lock().unwrap().push_str(&text)),
        )
        .await
        .unwrap();
        terminals
            .lock()
            .unwrap()
            .write("native-fixture", &print_command("CUI-空間-ready"))
            .unwrap();
        terminal_output(&output, "CUI-空間-ready");
        terminals
            .lock()
            .unwrap()
            .resize("native-fixture", 48, 120)
            .unwrap();
        terminals
            .lock()
            .unwrap()
            .write("native-fixture", "stty size\n")
            .unwrap();
        terminal_output(&output, "48 120");
        terminals
            .lock()
            .unwrap()
            .write(
                "native-fixture",
                &(print_command("sleep-ready").trim_end().to_string() + "; sleep 60\n"),
            )
            .unwrap();
        terminal_output(&output, "sleep-ready");
        terminals
            .lock()
            .unwrap()
            .write("native-fixture", "\u{3}")
            .unwrap();
        terminals
            .lock()
            .unwrap()
            .write("native-fixture", &print_command("after-interrupt"))
            .unwrap();
        terminal_output(&output, "after-interrupt");
        terminals
            .lock()
            .unwrap()
            .write("native-fixture", "exit\n")
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while terminals
            .lock()
            .unwrap()
            .poll_exit("native-fixture")
            .is_none()
        {
            assert!(Instant::now() < deadline, "terminal did not exit");
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(terminals.lock().unwrap().is_empty());
        terminals.lock().unwrap().close_all();
        assert_eq!(
            fixture.inspect("container", &id).unwrap()[0]["State"]["Running"],
            true,
            "closing terminal never stops its container"
        );

        let project = fixture.name("compose");
        let file = fixture.work.join("project 空間.yaml");
        std::fs::write(&file, format!("name: {project}\nservices:\n  fixture:\n    image: {tag}\n    pull_policy: never\n    labels:\n      io.colimaui.test.run: {}\n", fixture.run)).unwrap();
        fixture.claim_compose(&file);
        compose::compose_up(file.to_str().unwrap().into(), true)
            .await
            .unwrap();
        assert!(compose::list_compose_projects()
            .await
            .unwrap()
            .iter()
            .any(|value| value.name == project));
        assert!(compose::compose_ps(project.clone())
            .await
            .unwrap()
            .contains("fixture"));
        compose::compose_stop(project.clone()).await.unwrap();
        compose::compose_down(project).await.unwrap();

        let samples = metrics_collector::sample_now().await.unwrap();
        assert!(samples.iter().any(
            |sample| id.starts_with(&sample.container_id) && sample.instance == fixture.context
        ));
        containers::stop_container(id.clone()).await.unwrap();
        assert_eq!(
            fixture.inspect("container", &id).unwrap()[0]["State"]["Running"],
            false
        );
        containers::start_container(id.clone()).await.unwrap();
        containers::remove_container(id.clone(), true)
            .await
            .unwrap();
        assert!(!fixture
            .output(&["container", "inspect", &id])
            .unwrap()
            .status
            .success());
        networks::remove_network(network).await.unwrap();
        volumes::remove_volume(volume, false).await.unwrap();

        fixture.cleanup().unwrap();
        assert_eq!(
            fixture.inspect("image", &fixture.image).unwrap()[0]["Id"].as_str(),
            Some(fixture.image_id.as_str()),
            "external fixture image survives run-owned cleanup"
        );
        let receipt = fixture.work.parent().unwrap().join("boundary.json");
        std::fs::write(receipt, serde_json::to_vec_pretty(&serde_json::json!({
            "runId": fixture.run, "providerDeclared": fixture.provider, "context": fixture.context,
            "engineFingerprint": sha256(fixture.engine_id.as_bytes()),
            "result": "PASS", "scope": "application library boundaries on native engine; no installed GUI/IPC or complete canonical-case acceptance",
            "asserted": ["containers", "images", "volumes", "networks", "copy hash/TAR roundtrip", "image export/import", "Compose Unicode file", "ConPTY UTF8/resize/interrupt/exit", "terminal external ownership", "metrics attribution", "owned cleanup"],
            "qualification": "PENDING — remaining canonical variants, installed UI/IPC, privacy, recovery, performance, installer/updater and both-provider acceptance required"
        })).unwrap()).unwrap();
    }
}
