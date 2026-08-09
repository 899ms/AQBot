use aqbot_acp_client::config::ConfiguredAgent;
use aqbot_acp_client::proxy::{
    configured_agent_with_proxy, ProcessProxySettings, ProxyEnvironment,
};
use aqbot_acp_client::runtime::{
    AcpEvent, AcpInteractionKind, AcpInteractionOutcome, AcpQuestionnaireAnswer,
    AcpQuestionnaireOutcome, AcpQuestionnaireSubmission, AcpRuntime, RuntimeLimits,
    ACP_STATUS_GROK_RETRY_PREFIX, ACP_STATUS_SENDING_PROMPT,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

const FAKE_AGENT: &str = r##"
import json
import os
import sys
import time

log_path = sys.argv[1]
session_number = 0
pending_prompts = {}
pending_permissions = {}
pending_elicitations = {}
pending_plan_reviews = {}
pending_qwen_questions = {}
pending_claude_plans = {}
current_permission_mode = "unset"
supports_form_elicitation = False

if "help" in sys.argv[2:]:
    print('`model`:\n- "model-a"\n- "model-b"\n`next`:', flush=True)
    raise SystemExit(0)
if "--help" in sys.argv[2:]:
    print('--reasoning-effort <LEVEL> (choices: low, high)', flush=True)
    raise SystemExit(0)

def record(kind, detail=""):
    with open(log_path, "a", encoding="utf-8") as log:
        log.write(f"{kind}\t{os.getpid()}\t{detail}\n")
        log.flush()

def was_recorded(kind):
    try:
        with open(log_path, "r", encoding="utf-8") as log:
            return any(line.startswith(f"{kind}\t") for line in log)
    except FileNotFoundError:
        return False

def respond(request_id, result):
    print(json.dumps({"jsonrpc": "2.0", "id": request_id, "result": result}), flush=True)

def respond_error(request_id, message):
    print(json.dumps({
        "jsonrpc": "2.0",
        "id": request_id,
        "error": {"code": -32000, "message": message}
    }), flush=True)

proxy_keys = [
    "HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "NO_PROXY",
    "http_proxy", "https_proxy", "all_proxy", "no_proxy"
]
record("process", json.dumps({key: os.environ.get(key) for key in proxy_keys}, sort_keys=True))
for line in sys.stdin:
    message = json.loads(line)
    method = message.get("method")
    params = message.get("params") or {}
    if method is None and message.get("id") in pending_elicitations:
        prompt_id, session_id = pending_elicitations.pop(message["id"])
        record("elicitation/response", f"{session_id}:{json.dumps(message.get('result'), sort_keys=True)}")
        respond(prompt_id, {"stopReason": "end_turn"})
    elif method is None and message.get("id") in pending_plan_reviews:
        prompt_id, session_id = pending_plan_reviews.pop(message["id"])
        record("plan-review/response", f"{session_id}:{json.dumps(message.get('result'), sort_keys=True)}")
        respond(prompt_id, {"stopReason": "end_turn"})
    elif method is None and message.get("id") in pending_qwen_questions:
        prompt_id, session_id = pending_qwen_questions.pop(message["id"])
        record("qwen-question/response", f"{session_id}:{json.dumps(message.get('result'), sort_keys=True)}")
        respond(prompt_id, {"stopReason": "end_turn"})
    elif method is None and message.get("id") in pending_claude_plans:
        prompt_id, session_id = pending_claude_plans.pop(message["id"])
        record("claude-plan/response", f"{session_id}:{json.dumps(message.get('result'), sort_keys=True)}")
        respond(prompt_id, {"stopReason": "end_turn"})
    elif method is None and message.get("id") in pending_permissions:
        prompt_id, session_id = pending_permissions.pop(message["id"])
        record("permission/response", f"{session_id}:{json.dumps(message.get('result'), sort_keys=True)}")
        respond(prompt_id, {"stopReason": "end_turn"})
    elif method == "initialize":
        client_capabilities = params.get("clientCapabilities") or {}
        form_capabilities = (client_capabilities.get("elicitation") or {}).get("form")
        supports_form_elicitation = isinstance(form_capabilities, dict)
        record("initialize", json.dumps(client_capabilities, sort_keys=True))
        fail_replacement_initialize = (
            "fail-first-replacement-initialize" in sys.argv[2:]
            and "--reasoning-effort" in sys.argv[2:]
            and not was_recorded("initialize/replacement-fail")
        )
        if fail_replacement_initialize:
            record("initialize/replacement-fail")
            respond_error(message["id"], "forced replacement initialize failure")
            continue
        if (
            "delay-replacement-initialize" in sys.argv[2:]
            and "--reasoning-effort" in sys.argv[2:]
        ):
            record("initialize/replacement-delay")
            time.sleep(1.5)
        fail_shared_initialize = (
            "fail-first-shared-initialize" in sys.argv[2:]
            and not was_recorded("initialize/shared-fail")
        )
        if fail_shared_initialize:
            record("initialize/shared-fail")
            time.sleep(0.2)
            respond_error(message["id"], "forced shared initialize failure")
            continue
        result = {"protocolVersion": 1, "agentCapabilities": {}}
        if "supports-close" in sys.argv[2:]:
            result["agentCapabilities"] = {"sessionCapabilities": {"close": {}}}
        if "fake-grok" in sys.argv[2:]:
            result["_meta"] = {"grokShell": True}
        exit_after_initialize = (
            "exit-first-after-initialize" in sys.argv[2:]
            and not was_recorded("initialize/forced-exit")
        )
        if exit_after_initialize:
            record("initialize/forced-exit")
        respond(message["id"], result)
        if exit_after_initialize:
            time.sleep(0.1)
            break
    elif method == "session/new":
        if "hang-session-new" in sys.argv[2:]:
            record("session/new/hang")
            time.sleep(30)
            continue
        session_number += 1
        session_id = f"{os.getpid()}-session-{session_number}"
        record("session/new", session_id)
        respond(message["id"], {"sessionId": session_id})
    elif method == "session/prompt":
        session_id = params["sessionId"]
        prompt = params.get("prompt") or []
        prompt_text = next((block.get("text", "") for block in prompt if block.get("type") == "text"), "")
        record("session/prompt", f"{session_id}:{prompt_text}:permission={current_permission_mode}")
        if prompt_text == "config-update":
            print(json.dumps({
                "jsonrpc": "2.0",
                "method": "session/update",
                "params": {
                    "sessionId": session_id,
                    "update": {
                        "sessionUpdate": "config_option_update",
                        "configOptions": []
                    }
                }
            }), flush=True)
        if prompt_text == "retry":
            print(json.dumps({
                "jsonrpc": "2.0",
                "method": "_x.ai/session/update",
                "params": {
                    "sessionId": session_id,
                    "update": {
                        "sessionUpdate": "retry_state",
                        "attempt": 2,
                        "max_retries": 15,
                        "reason": "upstream timeout"
                    }
                }
            }), flush=True)
        print(json.dumps({
            "jsonrpc": "2.0",
            "method": "session/update",
            "params": {
                "sessionId": session_id,
                "update": {
                    "sessionUpdate": "agent_message_chunk",
                    "content": {"type": "text", "text": f"{session_id}:{prompt_text}"}
                }
            }
        }), flush=True)
        if prompt_text == "permission":
            permission_id = 100000 + session_number
            pending_permissions[permission_id] = (message["id"], session_id)
            print(json.dumps({
                "jsonrpc": "2.0",
                "id": permission_id,
                "method": "session/request_permission",
                "params": {
                    "sessionId": session_id,
                    "toolCall": {"toolCallId": f"tool-{session_id}", "title": "Edit file"},
                    "options": [
                        {"optionId": "allow-once", "name": "Allow", "kind": "allow_once"},
                        {"optionId": "reject-once", "name": "Reject", "kind": "reject_once"}
                    ]
                }
            }), flush=True)
        elif prompt_text == "codex-form-elicitation":
            if not supports_form_elicitation:
                record("elicitation/unsupported", session_id)
                respond(message["id"], {"stopReason": "end_turn"})
                continue
            elicitation_id = 300000 + session_number
            pending_elicitations[elicitation_id] = (message["id"], session_id)
            print(json.dumps({
                "jsonrpc": "2.0",
                "id": elicitation_id,
                "method": "elicitation/create",
                "params": {
                    "sessionId": session_id,
                    "toolCallId": f"question-{session_id}",
                    "mode": "form",
                    "message": "请选择工作范围并填写数量",
                    "requestedSchema": {
                        "type": "object",
                        "properties": {
                            "scope": {
                                "type": "string",
                                "title": "工作范围",
                                "description": "选择计划覆盖的范围",
                                "_meta": {
                                    "codex": {"isOther": True, "isSecret": False}
                                },
                                "oneOf": [
                                    {
                                        "const": "toolbar",
                                        "title": "仅工具栏",
                                        "description": "只处理工具栏"
                                    },
                                    {
                                        "const": "full-app",
                                        "title": "整个应用",
                                        "description": "覆盖整个应用"
                                    }
                                ]
                            },
                            "scope__other": {
                                "type": "string",
                                "title": "Other",
                                "description": "Type your own answer instead.",
                                "_meta": {
                                    "codex": {
                                        "questionId": "scope",
                                        "isOtherAnswer": True,
                                        "isSecret": False
                                    }
                                }
                            },
                            "variant_count": {
                                "type": "integer",
                                "title": "方案数量",
                                "description": "填写要比较的方案数量",
                                "minimum": 1,
                                "maximum": 5
                            }
                        },
                        "required": ["variant_count"]
                    },
                    "_meta": {"codex": {"autoResolutionMs": None}}
                }
            }), flush=True)
        elif prompt_text == "codex-plan-review":
            review_id = 400000 + session_number
            pending_plan_reviews[review_id] = (message["id"], session_id)
            print(json.dumps({
                "jsonrpc": "2.0",
                "id": review_id,
                "method": "session/request_permission",
                "params": {
                    "sessionId": session_id,
                    "toolCall": {
                        "toolCallId": f"plan-{session_id}",
                        "title": "Review plan",
                        "kind": "switch_mode",
                        "status": "pending",
                        "rawInput": {"plan": "# Test Plan\n\n- Step one"}
                    },
                    "options": [
                        {
                            "optionId": "implement_plan",
                            "name": "Implement the plan",
                            "kind": "allow_once"
                        },
                        {
                            "optionId": "revise_plan",
                            "name": "Revise the plan",
                            "kind": "reject_once"
                        }
                    ],
                    "_meta": {
                        "codex": {
                            "kind": "plan_review",
                            "planItemId": f"plan-{session_id}"
                        }
                    }
                }
            }), flush=True)
        elif prompt_text == "qwen-user-question":
            question_id = 500000 + session_number
            pending_qwen_questions[question_id] = (message["id"], session_id)
            questions = [
                {
                    "header": "Language",
                    "question": "Which language should the project use?",
                    "multiSelect": False,
                    "options": [
                        {
                            "label": "TypeScript",
                            "description": "Use TypeScript throughout."
                        },
                        {
                            "label": "Rust",
                            "description": "Use Rust throughout."
                        }
                    ]
                },
                {
                    "header": "Checks",
                    "question": "Which checks should be enabled?",
                    "multiSelect": True,
                    "options": [
                        {
                            "label": "Unit tests",
                            "description": "Run focused unit tests."
                        },
                        {
                            "label": "Lint",
                            "description": "Run the linter."
                        }
                    ]
                }
            ]
            print(json.dumps({
                "jsonrpc": "2.0",
                "id": question_id,
                "method": "session/request_permission",
                "params": {
                    "sessionId": session_id,
                    "toolCall": {
                        "toolCallId": f"qwen-question-{session_id}",
                        "status": "pending",
                        "title": "Ask user 2 questions",
                        "kind": "think",
                        "rawInput": {"questions": questions},
                        "_meta": {
                            "toolName": "ask_user_question",
                            "qwenInteractionKind": "user_question",
                            "qwenQuestions": questions
                        }
                    },
                    "options": [
                        {
                            "optionId": "proceed_once",
                            "name": "Submit",
                            "kind": "allow_once"
                        },
                        {
                            "optionId": "cancel",
                            "name": "Cancel",
                            "kind": "reject_once"
                        }
                    ]
                }
            }), flush=True)
        elif prompt_text == "claude-plan-review":
            review_id = 600000 + session_number
            pending_claude_plans[review_id] = (message["id"], session_id)
            print(json.dumps({
                "jsonrpc": "2.0",
                "id": review_id,
                "method": "session/request_permission",
                "params": {
                    "sessionId": session_id,
                    "toolCall": {
                        "toolCallId": f"claude-plan-{session_id}",
                        "title": "Ready to code?",
                        "kind": "switch_mode",
                        "status": "pending",
                        "rawInput": {"plan": "# Claude Plan\n\n- Keep the API stable"},
                        "content": [
                            {
                                "type": "content",
                                "content": {
                                    "type": "text",
                                    "text": "# Claude Plan\n\n- Keep the API stable"
                                }
                            }
                        ]
                    },
                    "options": [
                        {
                            "optionId": "acceptEdits",
                            "name": "Yes, and auto-accept edits",
                            "kind": "allow_always"
                        },
                        {
                            "optionId": "default",
                            "name": "Yes, and manually approve edits",
                            "kind": "allow_once"
                        },
                        {
                            "optionId": "plan",
                            "name": "No, keep planning",
                            "kind": "reject_once"
                        }
                    ]
                }
            }), flush=True)
        elif prompt_text in ("wait-for-cancel", "ignore-cancel", "cancel-then-permission"):
            pending_prompts[session_id] = (
                message["id"],
                prompt_text == "ignore-cancel",
                prompt_text
            )
        else:
            respond(message["id"], {"stopReason": "end_turn"})
    elif method == "session/close":
        record("session/close", params["sessionId"])
        if "fail-close" in sys.argv[2:]:
            respond_error(message["id"], "forced session close failure")
        else:
            respond(message["id"], {})
    elif method == "session/set_model":
        record("session/set_model", f"{params['sessionId']}:{params['modelId']}")
        respond(message["id"], {})
    elif method == "_x.ai/yolo_mode_changed":
        current_permission_mode = params.get("permission_mode", "missing")
        record("grok/permission", current_permission_mode)
    elif method == "session/cancel":
        session_id = params["sessionId"]
        record("session/cancel", session_id)
        pending = pending_prompts.pop(session_id, None)
        if pending is not None and pending[2] == "cancel-then-permission":
            permission_id = 200000 + session_number
            pending_permissions[permission_id] = (pending[0], session_id)
            print(json.dumps({
                "jsonrpc": "2.0",
                "id": permission_id,
                "method": "session/request_permission",
                "params": {
                    "sessionId": session_id,
                    "toolCall": {"toolCallId": f"late-tool-{session_id}", "title": "Late edit"},
                    "options": [
                        {"optionId": "allow-once", "name": "Allow", "kind": "allow_once"},
                        {"optionId": "reject-once", "name": "Reject", "kind": "reject_once"}
                    ]
                }
            }), flush=True)
        elif pending is not None and not pending[1]:
            prompt_id = pending[0]
            respond(prompt_id, {"stopReason": "cancelled"})
"##;

fn fake_agent(log_path: &Path) -> ConfiguredAgent {
    ConfiguredAgent {
        id: "fake-shared-agent".into(),
        name: "Fake shared agent".into(),
        enabled: true,
        source: "custom".into(),
        command: "python3".into(),
        args: vec![
            "-u".into(),
            "-c".into(),
            FAKE_AGENT.into(),
            log_path.to_string_lossy().into_owned(),
            "@github/copilot".into(),
            "--acp".into(),
        ],
        env: HashMap::new(),
        icon: None,
        sort: 0,
    }
}

fn fake_grok_agent(log_path: &Path) -> ConfiguredAgent {
    let mut agent = fake_agent(log_path);
    agent.id = "fake-grok-agent".into();
    agent.name = "Fake Grok agent".into();
    agent.args.truncate(4);
    agent.args.push("fake-grok".into());
    agent
}

fn fake_exit_once_agent(log_path: &Path) -> ConfiguredAgent {
    let mut agent = fake_agent(log_path);
    agent.args.push("exit-first-after-initialize".into());
    agent
}

fn fake_replacement_failure_agent(log_path: &Path) -> ConfiguredAgent {
    let mut agent = fake_agent(log_path);
    agent.args.push("fail-first-replacement-initialize".into());
    agent
}

fn fake_shared_startup_failure_agent(log_path: &Path) -> ConfiguredAgent {
    let mut agent = fake_agent(log_path);
    agent.args.push("fail-first-shared-initialize".into());
    agent
}

fn fake_slow_replacement_agent(log_path: &Path) -> ConfiguredAgent {
    let mut agent = fake_agent(log_path);
    agent.args.push("delay-replacement-initialize".into());
    agent
}

fn fake_hanging_session_new_agent(log_path: &Path) -> ConfiguredAgent {
    let mut agent = fake_agent(log_path);
    agent.args.push("hang-session-new".into());
    agent
}

fn unique_log_path(test_name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("aqbot-{test_name}-{}.log", uuid::Uuid::new_v4()))
}

fn events() -> mpsc::UnboundedSender<AcpEvent> {
    let (tx, _rx) = mpsc::unbounded_channel();
    tx
}

fn process_pids(log_path: &Path) -> Vec<u32> {
    std::fs::read_to_string(log_path)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| {
            let mut columns = line.split('\t');
            (columns.next() == Some("process"))
                .then(|| columns.next()?.parse::<u32>().ok())
                .flatten()
        })
        .collect()
}

fn process_proxy_environments(log_path: &Path) -> Vec<serde_json::Value> {
    std::fs::read_to_string(log_path)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| {
            let mut columns = line.splitn(3, '\t');
            (columns.next() == Some("process"))
                .then(|| {
                    columns
                        .nth(1)
                        .and_then(|raw| serde_json::from_str(raw).ok())
                })
                .flatten()
        })
        .collect()
}

fn assert_process_proxy_environment(
    environment: &serde_json::Value,
    expected_http: &str,
    expected_https: &str,
    expected_all: &str,
    expected_no_proxy: &str,
) {
    for (upper, lower, expected) in [
        ("HTTP_PROXY", "http_proxy", expected_http),
        ("HTTPS_PROXY", "https_proxy", expected_https),
        ("ALL_PROXY", "all_proxy", expected_all),
        ("NO_PROXY", "no_proxy", expected_no_proxy),
    ] {
        assert_eq!(environment[upper], expected, "{upper}: {environment}");
        assert_eq!(environment[lower], expected, "{lower}: {environment}");
    }
}

#[cfg(unix)]
fn process_is_running(pid: u32) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

#[cfg(unix)]
async fn wait_for_process_exit(pid: u32) {
    tokio::time::timeout(Duration::from_secs(2), async move {
        loop {
            if !process_is_running(pid) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("cancel timeout must terminate the abandoned ACP process");
}

fn prompt(text: &str) -> aqbot_acp_client::runtime::AcpPromptInput {
    aqbot_acp_client::runtime::AcpPromptInput {
        text: text.into(),
        attachments: Vec::new(),
    }
}

fn config_current(snapshot: &aqbot_acp_client::runtime::AcpSessionSnapshot, id: &str) -> String {
    let value = serde_json::to_value(snapshot).expect("serialize session snapshot");
    value["configOptions"]
        .as_array()
        .expect("config options")
        .iter()
        .find(|option| option["id"] == id)
        .and_then(|option| option["currentValue"].as_str())
        .unwrap_or_else(|| panic!("missing config option {id}: {value}"))
        .to_string()
}

#[tokio::test]
async fn system_proxy_environment_reaches_the_real_prewarmed_child_process() {
    let log_path = unique_log_path("system-proxy-prewarm");
    let runtime = AcpRuntime::new();
    let settings = ProcessProxySettings {
        proxy_type: Some("system".into()),
        address: None,
        port: None,
    };
    let system_proxy = ProxyEnvironment {
        http_proxy: Some("http://127.0.0.1:18080".into()),
        https_proxy: Some("http://127.0.0.1:18443".into()),
        all_proxy: Some("socks5://127.0.0.1:11080".into()),
        no_proxy: Some("localhost,127.0.0.1,.local".into()),
    };
    let agent =
        configured_agent_with_proxy(
            fake_agent(&log_path),
            &settings,
            || Ok(system_proxy.clone()),
        )
        .expect("resolve system proxy for ACP child");

    runtime
        .prewarm_agent(&agent, false, RuntimeLimits::new(60, 2))
        .await
        .expect("prewarm proxied fake Agent");

    let environments = process_proxy_environments(&log_path);
    assert_eq!(
        environments.len(),
        1,
        "expected one child process: {environments:?}"
    );
    assert_process_proxy_environment(
        &environments[0],
        "http://127.0.0.1:18080",
        "http://127.0.0.1:18443",
        "socks5://127.0.0.1:11080",
        "localhost,127.0.0.1,.local,::1",
    );
}

#[tokio::test]
async fn proxy_environment_is_stable_across_every_process_start_path() {
    #[derive(Clone, Copy)]
    enum StartPath {
        Prewarm,
        ColdPrepare,
        Recreate,
    }

    struct Case {
        name: &'static str,
        start: StartPath,
        proxy_type: Option<&'static str>,
        address: Option<&'static str>,
        port: Option<u16>,
        poison_agent_env: bool,
        expected_proxy: &'static str,
        expected_no_proxy: &'static str,
        expected_processes: usize,
    }

    let cases = [
        Case {
            name: "prewarm-system",
            start: StartPath::Prewarm,
            proxy_type: Some("system"),
            address: None,
            port: None,
            poison_agent_env: false,
            expected_proxy: "http://system.local:18080",
            expected_no_proxy: "localhost,.local,127.0.0.1,::1",
            expected_processes: 1,
        },
        Case {
            name: "cold-http",
            start: StartPath::ColdPrepare,
            proxy_type: Some("http"),
            address: Some("manual.local"),
            port: Some(28080),
            poison_agent_env: false,
            expected_proxy: "http://manual.local:28080",
            expected_no_proxy: "localhost,127.0.0.1,::1",
            expected_processes: 1,
        },
        Case {
            name: "recreate-socks5",
            start: StartPath::Recreate,
            proxy_type: Some("socks5"),
            address: Some("socks.local"),
            port: Some(21080),
            poison_agent_env: false,
            expected_proxy: "socks5://socks.local:21080",
            expected_no_proxy: "localhost,127.0.0.1,::1",
            expected_processes: 2,
        },
        Case {
            name: "none-clears-poison",
            start: StartPath::Prewarm,
            proxy_type: None,
            address: None,
            port: None,
            poison_agent_env: true,
            expected_proxy: "",
            expected_no_proxy: "*",
            expected_processes: 1,
        },
    ];

    for case in cases {
        let log_path = unique_log_path(case.name);
        let runtime = AcpRuntime::new();
        let mut source_agent = match case.start {
            StartPath::Recreate => fake_exit_once_agent(&log_path),
            _ => fake_agent(&log_path),
        };
        if case.poison_agent_env {
            for key in [
                "HTTP_PROXY",
                "HTTPS_PROXY",
                "ALL_PROXY",
                "NO_PROXY",
                "http_proxy",
                "https_proxy",
                "all_proxy",
                "no_proxy",
            ] {
                source_agent.env.insert(key.into(), "poison://proxy".into());
            }
        }
        let settings = ProcessProxySettings {
            proxy_type: case.proxy_type.map(str::to_string),
            address: case.address.map(str::to_string),
            port: case.port,
        };
        let system_proxy = ProxyEnvironment {
            http_proxy: Some("http://system.local:18080".into()),
            https_proxy: Some("http://system.local:18080".into()),
            all_proxy: Some("http://system.local:18080".into()),
            no_proxy: Some("localhost,.local".into()),
        };
        let agent =
            configured_agent_with_proxy(source_agent, &settings, || Ok(system_proxy.clone()))
                .unwrap_or_else(|error| panic!("{} proxy resolution failed: {error}", case.name));
        let limits = RuntimeLimits::new(60, 2);

        match case.start {
            StartPath::Prewarm => {
                runtime
                    .prewarm_agent(&agent, false, limits)
                    .await
                    .unwrap_or_else(|error| panic!("{} prewarm failed: {error}", case.name));
            }
            StartPath::ColdPrepare => {
                runtime
                    .prepare(
                        "proxy-cold-thread",
                        &agent,
                        std::env::current_dir().expect("current directory"),
                        None,
                        false,
                        limits,
                        events(),
                    )
                    .await
                    .unwrap_or_else(|error| panic!("{} prepare failed: {error}", case.name));
            }
            StartPath::Recreate => {
                runtime
                    .prewarm_agent(&agent, false, limits)
                    .await
                    .unwrap_or_else(|error| panic!("{} first prewarm failed: {error}", case.name));
                tokio::time::sleep(Duration::from_millis(300)).await;
                runtime
                    .prewarm_agent(&agent, false, limits)
                    .await
                    .unwrap_or_else(|error| panic!("{} recreate failed: {error}", case.name));
            }
        }

        let environments = process_proxy_environments(&log_path);
        assert_eq!(
            environments.len(),
            case.expected_processes,
            "{}: {environments:?}",
            case.name
        );
        for environment in &environments {
            assert_process_proxy_environment(
                environment,
                case.expected_proxy,
                case.expected_proxy,
                case.expected_proxy,
                case.expected_no_proxy,
            );
        }
        std::fs::remove_file(log_path).expect("remove fake agent log");
    }
}

#[tokio::test]
async fn prewarmed_process_hosts_multiple_thread_sessions() {
    let log_path = unique_log_path("shared-process");
    let runtime = Arc::new(AcpRuntime::new());
    let agent = fake_agent(&log_path);
    let limits = RuntimeLimits::new(60, 8);

    runtime
        .prewarm_agent(&agent, false, limits)
        .await
        .expect("prewarm fake agent");

    let prepare_a = runtime.prepare(
        "thread-a",
        &agent,
        std::env::current_dir().expect("current directory"),
        None,
        false,
        limits,
        events(),
    );
    let prepare_b = runtime.prepare(
        "thread-b",
        &agent,
        std::env::current_dir().expect("current directory"),
        None,
        false,
        limits,
        events(),
    );
    let (snapshot_a, snapshot_b) = tokio::join!(prepare_a, prepare_b);
    let snapshot_a = snapshot_a.expect("prepare thread-a");
    let snapshot_b = snapshot_b.expect("prepare thread-b");

    tokio::time::sleep(Duration::from_millis(150)).await;
    let log = std::fs::read_to_string(&log_path).expect("fake agent log");
    let initialize_count = log
        .lines()
        .filter(|line| line.starts_with("initialize\t"))
        .count();
    let new_session_count = log
        .lines()
        .filter(|line| line.starts_with("session/new\t"))
        .count();

    assert_eq!(
        initialize_count, 1,
        "one process per launch fingerprint\n{log}"
    );
    assert_eq!(new_session_count, 2, "one ACP session per thread\n{log}");
    assert_ne!(snapshot_a.session_id, snapshot_b.session_id);

    std::fs::remove_file(log_path).expect("remove fake agent log");
}

#[tokio::test]
async fn closing_one_supported_session_keeps_the_shared_session_usable() {
    let log_path = unique_log_path("session-close-isolation");
    let runtime = AcpRuntime::new();
    let mut agent = fake_agent(&log_path);
    agent.args.push("supports-close".into());
    let limits = RuntimeLimits::new(60, 8);
    let cwd = std::env::current_dir().expect("current directory");
    let snapshot_a = runtime
        .prepare(
            "thread-a",
            &agent,
            cwd.clone(),
            None,
            false,
            limits,
            events(),
        )
        .await
        .expect("prepare thread-a");
    let snapshot_b = runtime
        .prepare(
            "thread-b",
            &agent,
            cwd.clone(),
            None,
            false,
            limits,
            events(),
        )
        .await
        .expect("prepare thread-b");

    assert!(runtime
        .close_session("thread-a")
        .await
        .expect("close supported session"));
    assert!(!runtime.has_live_session("thread-a").await);
    assert!(runtime.has_live_session("thread-b").await);
    runtime
        .prompt(
            "thread-b",
            &agent,
            cwd,
            prompt("still-usable-after-a-close"),
            Some(snapshot_b.session_id),
            false,
            limits,
            events(),
        )
        .await
        .expect("prompt thread-b after closing thread-a");

    let log = std::fs::read_to_string(&log_path).expect("fake agent log");
    let closed = log
        .lines()
        .filter(|line| line.starts_with("session/close\t"))
        .collect::<Vec<_>>();
    assert_eq!(closed.len(), 1, "{log}");
    assert!(closed[0].ends_with(&snapshot_a.session_id), "{log}");
    assert!(log.contains("still-usable-after-a-close"), "{log}");

    std::fs::remove_file(log_path).expect("remove fake agent log");
}

#[tokio::test]
async fn closing_an_unsupported_session_only_detaches_local_state() {
    let log_path = unique_log_path("unsupported-session-close");
    let runtime = AcpRuntime::new();
    let agent = fake_agent(&log_path);
    let limits = RuntimeLimits::new(60, 8);
    runtime
        .prepare(
            "thread-a",
            &agent,
            std::env::current_dir().expect("current directory"),
            None,
            false,
            limits,
            events(),
        )
        .await
        .expect("prepare unsupported close session");

    assert!(runtime
        .close_session("thread-a")
        .await
        .expect("detach unsupported close session"));
    assert!(!runtime.has_live_session("thread-a").await);
    let log = std::fs::read_to_string(&log_path).expect("fake agent log");
    assert!(
        !log.lines().any(|line| line.starts_with("session/close\t")),
        "{log}"
    );

    std::fs::remove_file(log_path).expect("remove fake agent log");
}

#[tokio::test]
async fn failed_supported_close_is_observable_and_keeps_the_session_usable() {
    let log_path = unique_log_path("failed-session-close");
    let runtime = AcpRuntime::new();
    let mut agent = fake_agent(&log_path);
    agent.args.push("supports-close".into());
    agent.args.push("fail-close".into());
    let limits = RuntimeLimits::new(60, 8);
    let cwd = std::env::current_dir().expect("current directory");
    let snapshot = runtime
        .prepare(
            "thread-a",
            &agent,
            cwd.clone(),
            None,
            false,
            limits,
            events(),
        )
        .await
        .expect("prepare supported close session");

    let error = runtime
        .close_session("thread-a")
        .await
        .expect_err("agent close rejection must be returned");
    assert!(error.to_string().contains("forced session close failure"));
    assert!(runtime.has_live_session("thread-a").await);
    runtime
        .prompt(
            "thread-a",
            &agent,
            cwd,
            prompt("still-usable-after-close-rejection"),
            Some(snapshot.session_id),
            false,
            limits,
            events(),
        )
        .await
        .expect("prompt after close rejection");

    let log = std::fs::read_to_string(&log_path).expect("fake agent log");
    assert!(log.contains("still-usable-after-close-rejection"), "{log}");
    std::fs::remove_file(log_path).expect("remove fake agent log");
}

#[tokio::test]
async fn prewarm_reports_capacity_instead_of_claiming_a_second_agent_is_ready() {
    let log_path = unique_log_path("prewarm-capacity");
    let runtime = AcpRuntime::new();
    let first = fake_agent(&log_path);
    let mut second = first.clone();
    second.id = "second-fake-agent".into();
    second.name = "Second fake agent".into();
    let limits = RuntimeLimits::new(60, 1);

    assert!(runtime
        .prewarm_agent(&first, false, limits)
        .await
        .expect("prewarm first agent"));
    let error = runtime
        .prewarm_agent(&second, false, limits)
        .await
        .expect_err("second prewarm must report capacity");
    assert!(
        error
            .to_string()
            .contains("maximum concurrent ACP processes reached (1)"),
        "{error}"
    );

    let log = std::fs::read_to_string(&log_path).expect("fake agent log");
    assert_eq!(
        log.lines()
            .filter(|line| line.starts_with("process\t"))
            .count(),
        1,
        "{log}"
    );
    std::fs::remove_file(log_path).expect("remove fake agent log");
}

#[tokio::test]
async fn prewarm_replaces_an_anchor_that_exited_after_becoming_ready() {
    let log_path = unique_log_path("prewarm-recover-dead-anchor");
    let runtime = AcpRuntime::new();
    let agent = fake_exit_once_agent(&log_path);
    let limits = RuntimeLimits::new(60, 2);

    assert!(runtime
        .prewarm_agent(&agent, false, limits)
        .await
        .expect("first prewarm reaches ready"));
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(runtime
        .prewarm_agent(&agent, false, limits)
        .await
        .expect("second prewarm replaces exited process"));

    let log = std::fs::read_to_string(&log_path).expect("fake agent log");
    assert_eq!(
        log.lines()
            .filter(|line| line.starts_with("process\t"))
            .count(),
        2,
        "{log}"
    );
    std::fs::remove_file(log_path).expect("remove fake agent log");
}

#[tokio::test]
async fn failed_shared_startup_clears_every_logical_session_in_the_process_scope() {
    let log_path = unique_log_path("shared-startup-scope-cleanup");
    let runtime = Arc::new(AcpRuntime::new());
    let agent = fake_shared_startup_failure_agent(&log_path);
    let limits = RuntimeLimits::new(60, 4);
    let cwd = std::env::current_dir().expect("current directory");

    let (result_a, result_b) = tokio::join!(
        runtime.prepare(
            "thread-a",
            &agent,
            cwd.clone(),
            None,
            false,
            limits,
            events(),
        ),
        runtime.prepare(
            "thread-b",
            &agent,
            cwd.clone(),
            None,
            false,
            limits,
            events(),
        )
    );
    assert!(result_a.is_err(), "thread-a unexpectedly prepared");
    assert!(result_b.is_err(), "thread-b unexpectedly prepared");
    assert!(!runtime.has_live_session("thread-a").await);
    assert!(!runtime.has_live_session("thread-b").await);

    runtime
        .prepare("thread-c", &agent, cwd, None, false, limits, events())
        .await
        .expect("retry starts a fresh healthy process");
    let log = std::fs::read_to_string(&log_path).expect("fake agent log");
    assert_eq!(
        log.lines()
            .filter(|line| line.starts_with("process\t"))
            .count(),
        2,
        "{log}"
    );
    std::fs::remove_file(log_path).expect("remove fake agent log");
}

#[tokio::test]
async fn prepare_releases_its_event_stream_after_snapshot() {
    let log_path = unique_log_path("prepare-event-stream");
    let runtime = AcpRuntime::new();
    let agent = fake_agent(&log_path);
    let limits = RuntimeLimits::new(60, 8);
    let (event_tx, mut event_rx) = mpsc::unbounded_channel();

    runtime
        .prepare(
            "thread-a",
            &agent,
            std::env::current_dir().expect("current directory"),
            None,
            false,
            limits,
            event_tx,
        )
        .await
        .expect("prepare thread-a");

    tokio::time::timeout(Duration::from_secs(2), async {
        while event_rx.recv().await.is_some() {}
    })
    .await
    .expect("prepare event stream must close after queued events are drained");

    std::fs::remove_file(log_path).expect("remove fake agent log");
}

#[tokio::test]
async fn second_thread_prepare_is_not_blocked_by_a_running_prompt() {
    let log_path = unique_log_path("prepare-during-prompt");
    let runtime = AcpRuntime::new();
    let agent = fake_agent(&log_path);
    let limits = RuntimeLimits::new(60, 8);
    let cwd = std::env::current_dir().expect("current directory");
    let snapshot_a = runtime
        .prepare(
            "thread-a",
            &agent,
            cwd.clone(),
            None,
            false,
            limits,
            events(),
        )
        .await
        .expect("prepare thread-a");
    let (event_tx, mut event_rx) = mpsc::unbounded_channel();
    let handle = runtime
        .schedule_prompt(
            "thread-a",
            &agent,
            cwd.clone(),
            prompt("wait-for-cancel"),
            Some(snapshot_a.session_id),
            false,
            limits,
            event_tx,
        )
        .await
        .expect("schedule long thread-a prompt");
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if matches!(
                event_rx.recv().await,
                Some(AcpEvent::Status { message }) if message == ACP_STATUS_SENDING_PROMPT
            ) {
                break;
            }
        }
    })
    .await
    .expect("thread-a prompt started");

    tokio::time::timeout(
        Duration::from_secs(1),
        runtime.prepare("thread-b", &agent, cwd, None, false, limits, events()),
    )
    .await
    .expect("thread-b prepare must not wait for thread-a prompt")
    .expect("prepare thread-b");

    assert!(runtime.cancel("thread-a").await.expect("cancel thread-a"));
    handle.wait().await.expect("cancelled prompt completes");
    std::fs::remove_file(log_path).expect("remove fake agent log");
}

#[tokio::test]
async fn grok_permission_mode_is_replayed_for_each_logical_session_prompt() {
    let log_path = unique_log_path("grok-permission-isolation");
    let runtime = AcpRuntime::new();
    let agent = fake_grok_agent(&log_path);
    let limits = RuntimeLimits::new(60, 8);
    let cwd = std::env::current_dir().expect("current directory");
    let snapshot_a = runtime
        .prepare(
            "thread-a",
            &agent,
            cwd.clone(),
            None,
            false,
            limits,
            events(),
        )
        .await
        .expect("prepare thread-a");
    let snapshot_b = runtime
        .prepare(
            "thread-b",
            &agent,
            cwd.clone(),
            None,
            false,
            limits,
            events(),
        )
        .await
        .expect("prepare thread-b");

    let selected_a = runtime
        .set_config_option(
            "thread-a",
            "aqbot_grok_permission",
            serde_json::json!("bypassPermissions"),
        )
        .await
        .expect("set thread-a Grok permission");
    let unchanged_b = runtime
        .session_snapshot("thread-b")
        .await
        .expect("read thread-b")
        .expect("thread-b live");
    assert_eq!(
        config_current(&selected_a, "aqbot_grok_permission"),
        "bypassPermissions"
    );
    assert_eq!(
        config_current(&unchanged_b, "aqbot_grok_permission"),
        "default"
    );

    runtime
        .prompt(
            "thread-a",
            &agent,
            cwd.clone(),
            prompt("alpha"),
            Some(snapshot_a.session_id),
            false,
            limits,
            events(),
        )
        .await
        .expect("prompt thread-a");
    runtime
        .prompt(
            "thread-b",
            &agent,
            cwd,
            prompt("beta"),
            Some(snapshot_b.session_id),
            false,
            limits,
            events(),
        )
        .await
        .expect("prompt thread-b");

    let log = std::fs::read_to_string(&log_path).expect("fake agent log");
    let prompts = log
        .lines()
        .filter(|line| line.starts_with("session/prompt\t"))
        .collect::<Vec<_>>();
    assert_eq!(prompts.len(), 2, "{log}");
    assert!(
        prompts[0].ends_with("alpha:permission=always-approve"),
        "{log}"
    );
    assert!(prompts[1].ends_with("beta:permission=ask"), "{log}");

    std::fs::remove_file(log_path).expect("remove fake agent log");
}

#[tokio::test]
async fn grok_permission_changes_do_not_mutate_another_running_turn() {
    let log_path = unique_log_path("grok-permission-running-isolation");
    let runtime = AcpRuntime::new();
    let agent = fake_grok_agent(&log_path);
    let limits = RuntimeLimits::new(60, 8);
    let cwd = std::env::current_dir().expect("current directory");
    let snapshot_a = runtime
        .prepare(
            "thread-a",
            &agent,
            cwd.clone(),
            None,
            false,
            limits,
            events(),
        )
        .await
        .expect("prepare thread-a");
    let snapshot_b = runtime
        .prepare(
            "thread-b",
            &agent,
            cwd.clone(),
            None,
            false,
            limits,
            events(),
        )
        .await
        .expect("prepare thread-b");
    runtime
        .set_config_option(
            "thread-a",
            "aqbot_grok_permission",
            serde_json::json!("bypassPermissions"),
        )
        .await
        .expect("select thread-a bypass");
    let (events_a, mut received_a) = mpsc::unbounded_channel();
    let handle_a = runtime
        .schedule_prompt(
            "thread-a",
            &agent,
            cwd.clone(),
            prompt("wait-for-cancel"),
            Some(snapshot_a.session_id),
            false,
            limits,
            events_a,
        )
        .await
        .expect("start thread-a");
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if matches!(
                received_a.recv().await,
                Some(AcpEvent::Status { message }) if message == ACP_STATUS_SENDING_PROMPT
            ) {
                break;
            }
        }
    })
    .await
    .expect("thread-a prompt started");
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if std::fs::read_to_string(&log_path)
                .is_ok_and(|log| log.contains("wait-for-cancel:permission=always-approve"))
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("fake agent observed thread-a prompt");

    runtime
        .set_config_option(
            "thread-b",
            "aqbot_grok_permission",
            serde_json::json!("auto"),
        )
        .await
        .expect("update thread-b desired permission");
    let during_a = std::fs::read_to_string(&log_path).expect("fake agent log");
    assert_eq!(
        during_a
            .lines()
            .filter(|line| line.starts_with("grok/permission\t"))
            .count(),
        1,
        "thread-b selection changed process permission during thread-a turn:\n{during_a}"
    );

    assert!(runtime.cancel("thread-a").await.expect("cancel thread-a"));
    handle_a
        .wait()
        .await
        .expect("thread-a cancellation completes");
    runtime
        .prompt(
            "thread-b",
            &agent,
            cwd,
            prompt("beta"),
            Some(snapshot_b.session_id),
            false,
            limits,
            events(),
        )
        .await
        .expect("prompt thread-b");
    let final_log = std::fs::read_to_string(&log_path).expect("fake agent log");
    let permissions = final_log
        .lines()
        .filter(|line| line.starts_with("grok/permission\t"))
        .collect::<Vec<_>>();
    assert_eq!(permissions.len(), 2, "{final_log}");
    assert!(permissions[0].ends_with("always-approve"), "{final_log}");
    assert!(permissions[1].ends_with("auto"), "{final_log}");

    std::fs::remove_file(log_path).expect("remove fake agent log");
}

#[tokio::test]
async fn notifications_are_routed_to_their_thread_session() {
    let log_path = unique_log_path("notification-routing");
    let runtime = Arc::new(AcpRuntime::new());
    let agent = fake_agent(&log_path);
    let limits = RuntimeLimits::new(60, 8);
    let cwd = std::env::current_dir().expect("current directory");
    let (events_a, mut received_a) = mpsc::unbounded_channel();
    let (events_b, mut received_b) = mpsc::unbounded_channel();

    let prompt_a = runtime.prompt(
        "thread-a",
        &agent,
        cwd.clone(),
        prompt("alpha"),
        None,
        false,
        limits,
        events_a,
    );
    let prompt_b = runtime.prompt(
        "thread-b",
        &agent,
        cwd,
        prompt("beta"),
        None,
        false,
        limits,
        events_b,
    );
    let (outcome_a, outcome_b) = tokio::join!(prompt_a, prompt_b);
    let outcome_a = outcome_a.expect("prompt thread-a");
    let outcome_b = outcome_b.expect("prompt thread-b");

    let mut text_a = Vec::new();
    while let Ok(event) = received_a.try_recv() {
        if let AcpEvent::StreamText { text } = event {
            text_a.push(text);
        }
    }
    let mut text_b = Vec::new();
    while let Ok(event) = received_b.try_recv() {
        if let AcpEvent::StreamText { text } = event {
            text_b.push(text);
        }
    }

    assert_eq!(text_a, [format!("{}:alpha", outcome_a.session_id)]);
    assert_eq!(text_b, [format!("{}:beta", outcome_b.session_id)]);
    assert_ne!(outcome_a.session_id, outcome_b.session_id);

    std::fs::remove_file(log_path).expect("remove fake agent log");
}

#[tokio::test]
async fn launch_catalog_selection_is_isolated_per_thread() {
    let log_path = unique_log_path("config-isolation");
    let runtime = AcpRuntime::new();
    let agent = fake_agent(&log_path);
    let limits = RuntimeLimits::new(60, 8);
    let cwd = std::env::current_dir().expect("current directory");
    runtime
        .prepare(
            "thread-a",
            &agent,
            cwd.clone(),
            None,
            false,
            limits,
            events(),
        )
        .await
        .expect("prepare thread-a");
    runtime
        .prepare(
            "thread-b",
            &agent,
            cwd.clone(),
            None,
            false,
            limits,
            events(),
        )
        .await
        .expect("prepare thread-b");
    runtime
        .wait_for_capability_discovery("thread-a")
        .await
        .expect("discover launch catalog for thread-a");
    runtime
        .wait_for_capability_discovery("thread-b")
        .await
        .expect("discover launch catalog for thread-b");
    let before_b = runtime
        .session_snapshot("thread-b")
        .await
        .expect("read initial thread-b snapshot")
        .expect("thread-b is live");
    let expected_b_model = config_current(&before_b, "model");

    let changed_a = runtime
        .set_config_option("thread-a", "model", serde_json::json!("model-b"))
        .await
        .expect("change thread-a model");
    assert_eq!(config_current(&changed_a, "model"), "model-b");

    runtime
        .prompt(
            "thread-b",
            &agent,
            cwd,
            prompt("config-update"),
            None,
            false,
            limits,
            events(),
        )
        .await
        .expect("refresh thread-b config");
    let snapshot_b = runtime
        .session_snapshot("thread-b")
        .await
        .expect("read thread-b snapshot")
        .expect("thread-b is live");

    assert_eq!(config_current(&snapshot_b, "model"), expected_b_model);

    std::fs::remove_file(log_path).expect("remove fake agent log");
}

#[tokio::test]
async fn spawn_config_replacement_respects_capacity_and_preserves_original_session() {
    let log_path = unique_log_path("spawn-config-capacity");
    let runtime = AcpRuntime::new();
    let agent = fake_agent(&log_path);
    let limits = RuntimeLimits::new(60, 1);
    let cwd = std::env::current_dir().expect("current directory");
    let snapshot_a = runtime
        .prepare(
            "thread-a",
            &agent,
            cwd.clone(),
            None,
            false,
            limits,
            events(),
        )
        .await
        .expect("prepare thread-a");
    runtime
        .prepare(
            "thread-b",
            &agent,
            cwd.clone(),
            None,
            false,
            limits,
            events(),
        )
        .await
        .expect("prepare thread-b");
    runtime
        .wait_for_capability_discovery("thread-a")
        .await
        .expect("discover thread-a launch config");

    let error = runtime
        .set_config_option("thread-a", "reasoning_effort", serde_json::json!("high"))
        .await
        .expect_err("replacement must respect max process capacity");
    assert!(
        error
            .to_string()
            .contains("maximum concurrent ACP processes reached (1)"),
        "{error}"
    );
    let preserved = runtime
        .session_snapshot("thread-a")
        .await
        .expect("read original thread-a after failed replacement")
        .expect("thread-a remains live");
    assert_eq!(preserved.session_id, snapshot_a.session_id);
    runtime
        .prompt(
            "thread-a",
            &agent,
            cwd,
            prompt("still-usable"),
            Some(preserved.session_id),
            false,
            limits,
            events(),
        )
        .await
        .expect("original thread-a remains promptable");

    let log = std::fs::read_to_string(&log_path).expect("fake agent log");
    assert_eq!(
        log.lines()
            .filter(|line| line.starts_with("process\t"))
            .count(),
        1,
        "{log}"
    );
    std::fs::remove_file(log_path).expect("remove fake agent log");
}

#[tokio::test]
async fn failed_spawn_config_replacement_rolls_back_and_the_next_retry_uses_a_new_process() {
    let log_path = unique_log_path("spawn-config-init-rollback");
    let runtime = AcpRuntime::new();
    let agent = fake_replacement_failure_agent(&log_path);
    let limits = RuntimeLimits::new(60, 2);
    let cwd = std::env::current_dir().expect("current directory");
    let original = runtime
        .prepare(
            "thread-a",
            &agent,
            cwd.clone(),
            None,
            false,
            limits,
            events(),
        )
        .await
        .expect("prepare original thread-a");
    runtime
        .wait_for_capability_discovery("thread-a")
        .await
        .expect("discover spawn config");

    let error = runtime
        .set_config_option("thread-a", "reasoning_effort", serde_json::json!("high"))
        .await
        .expect_err("first replacement initialize is forced to fail");
    assert!(error.to_string().contains("initialize failed"), "{error}");
    let preserved = runtime
        .session_snapshot("thread-a")
        .await
        .expect("read original after failed replacement")
        .expect("original session remains live");
    assert_eq!(preserved.session_id, original.session_id);
    runtime
        .prompt(
            "thread-a",
            &agent,
            cwd,
            prompt("after-failed-replacement"),
            Some(preserved.session_id),
            false,
            limits,
            events(),
        )
        .await
        .expect("original session remains promptable");

    let replacement = runtime
        .set_config_option("thread-a", "reasoning_effort", serde_json::json!("high"))
        .await
        .expect("second replacement starts a fresh process");
    assert_eq!(config_current(&replacement, "reasoning_effort"), "high");

    let log = std::fs::read_to_string(&log_path).expect("fake agent log");
    assert_eq!(
        log.lines()
            .filter(|line| line.starts_with("process\t"))
            .count(),
        3,
        "{log}"
    );
    std::fs::remove_file(log_path).expect("remove fake agent log");
}

#[tokio::test]
async fn slow_replacement_startup_does_not_block_another_agent_prepare() {
    let log_path = unique_log_path("replacement-no-global-hol");
    let runtime = Arc::new(AcpRuntime::new());
    let agent = fake_slow_replacement_agent(&log_path);
    let limits = RuntimeLimits::new(60, 3);
    let cwd = std::env::current_dir().expect("current directory");
    runtime
        .prepare(
            "thread-a",
            &agent,
            cwd.clone(),
            None,
            false,
            limits,
            events(),
        )
        .await
        .expect("prepare thread-a");
    runtime
        .wait_for_capability_discovery("thread-a")
        .await
        .expect("discover spawn config");
    let replacement_runtime = runtime.clone();
    let replacement = tokio::spawn(async move {
        replacement_runtime
            .set_config_option("thread-a", "reasoning_effort", serde_json::json!("high"))
            .await
    });
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if std::fs::read_to_string(&log_path)
                .is_ok_and(|log| log.contains("initialize/replacement-delay"))
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("replacement initialize started");

    let mut other = fake_agent(&log_path);
    other.id = "independent-agent".into();
    other.name = "Independent agent".into();
    tokio::time::timeout(
        Duration::from_millis(500),
        runtime.prepare("thread-b", &other, cwd, None, false, limits, events()),
    )
    .await
    .expect("independent prepare must not wait for replacement initialize")
    .expect("prepare independent agent");
    replacement
        .await
        .expect("replacement task joins")
        .expect("replacement succeeds");

    std::fs::remove_file(log_path).expect("remove fake agent log");
}

#[tokio::test]
async fn replacement_retirement_prevents_attaching_to_the_evicted_process() {
    let log_path = unique_log_path("replacement-retirement");
    let runtime = Arc::new(AcpRuntime::new());
    let agent = fake_slow_replacement_agent(&log_path);
    let limits = RuntimeLimits::new(60, 1);
    let cwd = std::env::current_dir().expect("current directory");
    runtime
        .prepare(
            "thread-a",
            &agent,
            cwd.clone(),
            None,
            false,
            limits,
            events(),
        )
        .await
        .expect("prepare original thread-a");
    runtime
        .wait_for_capability_discovery("thread-a")
        .await
        .expect("discover spawn config");
    let original_pid = process_pids(&log_path)[0];

    let replacement_runtime = runtime.clone();
    let replacement = tokio::spawn(async move {
        replacement_runtime
            .set_config_option("thread-a", "reasoning_effort", serde_json::json!("high"))
            .await
    });
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if std::fs::read_to_string(&log_path)
                .is_ok_and(|log| log.contains("initialize/replacement-delay"))
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("replacement initialize started");

    let attach = tokio::time::timeout(
        Duration::from_millis(500),
        runtime.prepare("thread-b", &agent, cwd, None, false, limits, events()),
    )
    .await
    .expect("retiring process admission must not wait for replacement")
    .expect_err("thread-b must not attach to the retiring process");
    assert!(
        attach
            .to_string()
            .contains("maximum concurrent ACP processes reached (1)"),
        "{attach}"
    );
    replacement
        .await
        .expect("replacement task joins")
        .expect("replacement succeeds");
    assert!(!runtime.has_live_session("thread-b").await);

    let pids = process_pids(&log_path);
    assert_eq!(
        pids.len(),
        2,
        "{}",
        std::fs::read_to_string(&log_path).unwrap()
    );
    #[cfg(unix)]
    {
        wait_for_process_exit(original_pid).await;
        assert!(
            process_is_running(pids[1]),
            "replacement process exited early"
        );
    }
    std::fs::remove_file(log_path).expect("remove fake agent log");
}

#[tokio::test]
async fn failed_replacement_restores_evicted_anchor_and_preserves_capacity() {
    let log_path = unique_log_path("replacement-eviction-rollback");
    let runtime = AcpRuntime::new();
    let agent = fake_replacement_failure_agent(&log_path);
    let limits = RuntimeLimits::new(60, 1);
    let cwd = std::env::current_dir().expect("current directory");
    let original = runtime
        .prepare(
            "thread-a",
            &agent,
            cwd.clone(),
            None,
            false,
            limits,
            events(),
        )
        .await
        .expect("prepare original thread-a");
    runtime
        .wait_for_capability_discovery("thread-a")
        .await
        .expect("discover spawn config");
    runtime
        .set_config_option("thread-a", "reasoning_effort", serde_json::json!("high"))
        .await
        .expect_err("replacement initialize is forced to fail");

    let mut other = fake_agent(&log_path);
    other.id = "capacity-probe-agent".into();
    other.name = "Capacity probe agent".into();
    let capacity = runtime
        .prepare(
            "thread-c",
            &other,
            cwd.clone(),
            None,
            false,
            limits,
            events(),
        )
        .await
        .expect_err("restored old anchor must continue occupying max=1");
    assert!(
        capacity
            .to_string()
            .contains("maximum concurrent ACP processes reached (1)"),
        "{capacity}"
    );
    runtime
        .prompt(
            "thread-a",
            &agent,
            cwd,
            prompt("old-still-counted-and-usable"),
            Some(original.session_id),
            false,
            limits,
            events(),
        )
        .await
        .expect("old session remains usable");
    let log = std::fs::read_to_string(&log_path).expect("fake agent log");
    assert_eq!(
        log.lines()
            .filter(|line| line.starts_with("process\t"))
            .count(),
        2,
        "{log}"
    );

    std::fs::remove_file(log_path).expect("remove fake agent log");
}

#[tokio::test]
async fn grok_retry_extension_is_routed_by_session_id() {
    let log_path = unique_log_path("grok-retry-routing");
    let runtime = Arc::new(AcpRuntime::new());
    let agent = fake_agent(&log_path);
    let limits = RuntimeLimits::new(60, 8);
    let cwd = std::env::current_dir().expect("current directory");
    let (events_a, mut received_a) = mpsc::unbounded_channel();
    let (events_b, mut received_b) = mpsc::unbounded_channel();

    let (outcome_a, outcome_b) = tokio::join!(
        runtime.prompt(
            "thread-a",
            &agent,
            cwd.clone(),
            prompt("retry"),
            None,
            false,
            limits,
            events_a,
        ),
        runtime.prompt(
            "thread-b",
            &agent,
            cwd,
            prompt("beta"),
            None,
            false,
            limits,
            events_b,
        )
    );
    outcome_a.expect("prompt thread-a");
    outcome_b.expect("prompt thread-b");

    let retry_a = std::iter::from_fn(|| received_a.try_recv().ok())
        .filter_map(|event| match event {
            AcpEvent::Status { message } if message.starts_with(ACP_STATUS_GROK_RETRY_PREFIX) => {
                Some(message)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    let retry_b = std::iter::from_fn(|| received_b.try_recv().ok())
        .filter_map(|event| match event {
            AcpEvent::Status { message } if message.starts_with(ACP_STATUS_GROK_RETRY_PREFIX) => {
                Some(message)
            }
            _ => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(
        retry_a,
        [r#"aqbot:grok-retry:{"attempt":2,"maximum":15,"detail":"upstream timeout"}"#]
    );
    assert!(
        retry_b.is_empty(),
        "thread-b received thread-a retry: {retry_b:?}"
    );

    std::fs::remove_file(log_path).expect("remove fake agent log");
}

#[tokio::test]
async fn cancel_targets_only_the_requested_thread_session() {
    let log_path = unique_log_path("cancel-isolation");
    let runtime = AcpRuntime::new();
    let agent = fake_agent(&log_path);
    let limits = RuntimeLimits::new(60, 8);
    let cwd = std::env::current_dir().expect("current directory");
    let (events_a, mut received_a) = mpsc::unbounded_channel();
    let snapshot_a = runtime
        .prepare(
            "thread-a",
            &agent,
            cwd.clone(),
            None,
            false,
            limits,
            events(),
        )
        .await
        .expect("prepare thread-a");
    let snapshot_b = runtime
        .prepare(
            "thread-b",
            &agent,
            cwd.clone(),
            None,
            false,
            limits,
            events(),
        )
        .await
        .expect("prepare thread-b");
    let handle = runtime
        .schedule_prompt(
            "thread-a",
            &agent,
            cwd,
            prompt("wait-for-cancel"),
            Some(snapshot_a.session_id.clone()),
            false,
            limits,
            events_a,
        )
        .await
        .expect("schedule thread-a prompt");

    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if matches!(
                received_a.recv().await,
                Some(AcpEvent::Status { message }) if message == ACP_STATUS_SENDING_PROMPT
            ) {
                break;
            }
        }
    })
    .await
    .expect("thread-a prompt started");

    assert!(!runtime.cancel("thread-b").await.expect("cancel thread-b"));
    assert!(runtime.cancel("thread-a").await.expect("cancel thread-a"));
    handle.wait().await.expect("cancelled prompt completes");
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if std::fs::read_to_string(&log_path).is_ok_and(|log| log.contains("session/cancel\t"))
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("fake agent observed session/cancel");

    let log = std::fs::read_to_string(&log_path).expect("fake agent log");
    let cancelled = log
        .lines()
        .filter(|line| line.starts_with("session/cancel\t"))
        .collect::<Vec<_>>();
    assert_eq!(cancelled.len(), 1, "{log}");
    assert!(cancelled[0].ends_with(&snapshot_a.session_id), "{log}");
    assert!(!cancelled[0].ends_with(&snapshot_b.session_id), "{log}");

    std::fs::remove_file(log_path).expect("remove fake agent log");
}

#[tokio::test]
async fn reverse_permission_sent_after_cancel_never_reaches_the_ui() {
    let log_path = unique_log_path("cancel-late-reverse-permission");
    let runtime = AcpRuntime::new();
    let agent = fake_agent(&log_path);
    let limits = RuntimeLimits::new(60, 8);
    let cwd = std::env::current_dir().expect("current directory");
    let snapshot = runtime
        .prepare(
            "thread-a",
            &agent,
            cwd.clone(),
            None,
            false,
            limits,
            events(),
        )
        .await
        .expect("prepare thread-a");
    let (event_tx, mut event_rx) = mpsc::unbounded_channel();
    let handle = runtime
        .schedule_prompt(
            "thread-a",
            &agent,
            cwd,
            prompt("cancel-then-permission"),
            Some(snapshot.session_id),
            false,
            limits,
            event_tx,
        )
        .await
        .expect("schedule prompt");
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if matches!(
                event_rx.recv().await,
                Some(AcpEvent::Status { message }) if message == ACP_STATUS_SENDING_PROMPT
            ) {
                break;
            }
        }
    })
    .await
    .expect("prompt started");

    assert!(runtime.cancel("thread-a").await.expect("cancel thread-a"));
    let outcome = handle.wait().await.expect("cancelled prompt settles");
    assert_eq!(outcome.stop_reason, "cancelled");
    let remaining = std::iter::from_fn(|| event_rx.try_recv().ok()).collect::<Vec<_>>();
    assert!(
        remaining.iter().all(|event| !matches!(
            event,
            AcpEvent::PermissionRequest { .. } | AcpEvent::Plan { .. }
        )),
        "late reverse request leaked to UI: {remaining:?}"
    );
    let log = std::fs::read_to_string(&log_path).expect("fake agent log");
    assert!(log.contains("permission/response\t"), "{log}");
    assert!(log.contains("cancelled"), "{log}");
    std::fs::remove_file(log_path).expect("remove fake agent log");
}

#[tokio::test]
async fn cancelling_a_queued_thread_prevents_its_prompt_from_being_sent() {
    let log_path = unique_log_path("cancel-queued-prompt");
    let runtime = AcpRuntime::new();
    let agent = fake_agent(&log_path);
    let limits = RuntimeLimits::new(60, 8);
    let cwd = std::env::current_dir().expect("current directory");
    let snapshot_a = runtime
        .prepare(
            "thread-a",
            &agent,
            cwd.clone(),
            None,
            false,
            limits,
            events(),
        )
        .await
        .expect("prepare thread-a");
    let snapshot_b = runtime
        .prepare(
            "thread-b",
            &agent,
            cwd.clone(),
            None,
            false,
            limits,
            events(),
        )
        .await
        .expect("prepare thread-b");
    let (events_a, mut received_a) = mpsc::unbounded_channel();
    let handle_a = runtime
        .schedule_prompt(
            "thread-a",
            &agent,
            cwd.clone(),
            prompt("wait-for-cancel"),
            Some(snapshot_a.session_id.clone()),
            false,
            limits,
            events_a,
        )
        .await
        .expect("schedule thread-a");
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if matches!(
                received_a.recv().await,
                Some(AcpEvent::Status { message }) if message == ACP_STATUS_SENDING_PROMPT
            ) {
                break;
            }
        }
    })
    .await
    .expect("thread-a prompt started");
    let handle_b = runtime
        .schedule_prompt(
            "thread-b",
            &agent,
            cwd,
            prompt("must-not-send"),
            Some(snapshot_b.session_id.clone()),
            false,
            limits,
            events(),
        )
        .await
        .expect("queue thread-b");

    assert!(runtime
        .cancel("thread-b")
        .await
        .expect("cancel queued thread-b"));
    let outcome_b = tokio::time::timeout(Duration::from_secs(1), handle_b.wait())
        .await
        .expect("queued cancellation must complete while thread-a is still running")
        .expect("queued cancellation completes");
    assert_eq!(outcome_b.stop_reason, "cancelled");
    assert!(runtime
        .cancel("thread-a")
        .await
        .expect("cancel running thread-a"));
    handle_a
        .wait()
        .await
        .expect("thread-a cancellation completes");

    let log = std::fs::read_to_string(&log_path).expect("fake agent log");
    assert!(!log.contains("must-not-send"), "{log}");
    let cancelled = log
        .lines()
        .filter(|line| line.starts_with("session/cancel\t"))
        .collect::<Vec<_>>();
    assert_eq!(cancelled.len(), 1, "{log}");
    assert!(cancelled[0].ends_with(&snapshot_a.session_id), "{log}");
    assert!(!cancelled[0].ends_with(&snapshot_b.session_id), "{log}");

    std::fs::remove_file(log_path).expect("remove fake agent log");
}

#[tokio::test]
async fn cancelling_while_session_new_hangs_tears_down_the_process_scope() {
    let log_path = unique_log_path("cancel-hanging-session-new");
    let runtime = AcpRuntime::new();
    let agent = fake_hanging_session_new_agent(&log_path);
    let limits = RuntimeLimits::new(60, 8);
    let cwd = std::env::current_dir().expect("current directory");
    let handle = runtime
        .schedule_prompt(
            "thread-a",
            &agent,
            cwd,
            prompt("must-not-reach-session-prompt"),
            None,
            false,
            limits,
            events(),
        )
        .await
        .expect("schedule prompt");
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if std::fs::read_to_string(&log_path).is_ok_and(|log| log.contains("session/new/hang"))
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("fake agent entered session/new");
    let original_pid = process_pids(&log_path)[0];

    assert!(
        tokio::time::timeout(Duration::from_secs(4), runtime.cancel("thread-a"))
            .await
            .expect("session/new cancellation has a bounded teardown")
            .expect("cancel handled")
    );
    let cancelled = tokio::time::timeout(Duration::from_secs(1), handle.wait())
        .await
        .expect("session/new waiter settles after teardown")
        .expect("cancelled session/new maps to a cancelled outcome");
    assert_eq!(cancelled.stop_reason, "cancelled");
    assert!(!runtime.has_live_session("thread-a").await);
    #[cfg(unix)]
    wait_for_process_exit(original_pid).await;
    let log = std::fs::read_to_string(&log_path).expect("fake agent log");
    assert!(!log.contains("session/prompt\t"), "{log}");
    std::fs::remove_file(log_path).expect("remove fake agent log");
}

#[tokio::test]
async fn ignored_running_cancel_tears_down_scope_and_restarts_the_process() {
    let log_path = unique_log_path("cancel-ignored-by-agent");
    let runtime = AcpRuntime::new();
    let agent = fake_agent(&log_path);
    let limits = RuntimeLimits::new(60, 8);
    let cwd = std::env::current_dir().expect("current directory");
    let snapshot_a = runtime
        .prepare(
            "thread-a",
            &agent,
            cwd.clone(),
            None,
            false,
            limits,
            events(),
        )
        .await
        .expect("prepare thread-a");
    let _snapshot_b = runtime
        .prepare(
            "thread-b",
            &agent,
            cwd.clone(),
            None,
            false,
            limits,
            events(),
        )
        .await
        .expect("prepare thread-b");
    let original_pid = *process_pids(&log_path)
        .first()
        .expect("shared ACP process pid");
    let (events_a, mut received_a) = mpsc::unbounded_channel();
    let handle_a = runtime
        .schedule_prompt(
            "thread-a",
            &agent,
            cwd.clone(),
            prompt("ignore-cancel"),
            Some(snapshot_a.session_id),
            false,
            limits,
            events_a,
        )
        .await
        .expect("start ignored-cancel prompt");
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if matches!(
                received_a.recv().await,
                Some(AcpEvent::Status { message }) if message == ACP_STATUS_SENDING_PROMPT
            ) {
                break;
            }
        }
    })
    .await
    .expect("thread-a prompt started");

    assert!(
        tokio::time::timeout(Duration::from_secs(4), runtime.cancel("thread-a"))
            .await
            .expect("ignored cancellation must have a bounded teardown")
            .expect("cancel thread-a")
    );
    let cancelled = tokio::time::timeout(Duration::from_secs(1), handle_a.wait())
        .await
        .expect("process teardown must settle the cancelled prompt")
        .expect("local cancellation completes");
    assert_eq!(cancelled.stop_reason, "cancelled");
    assert!(!runtime.has_live_session("thread-a").await);
    assert!(!runtime.has_live_session("thread-b").await);
    #[cfg(unix)]
    wait_for_process_exit(original_pid).await;

    runtime
        .prompt(
            "thread-b",
            &agent,
            cwd,
            prompt("after-ignored-cancel"),
            None,
            false,
            limits,
            events(),
        )
        .await
        .expect("next prompt starts a clean ACP process");
    let log = std::fs::read_to_string(&log_path).expect("fake agent log");
    assert!(log.contains("after-ignored-cancel"), "{log}");
    let pids = process_pids(&log_path);
    assert_eq!(pids.len(), 2, "{log}");
    assert_ne!(pids[0], pids[1], "{log}");

    std::fs::remove_file(log_path).expect("remove fake agent log");
}

#[tokio::test]
async fn permission_request_is_routed_to_its_thread_session() {
    let log_path = unique_log_path("permission-routing");
    let runtime = AcpRuntime::new();
    let agent = fake_agent(&log_path);
    let limits = RuntimeLimits::new(60, 8);
    let cwd = std::env::current_dir().expect("current directory");
    let (events_a, mut received_a) = mpsc::unbounded_channel();
    let (events_b, mut received_b) = mpsc::unbounded_channel::<AcpEvent>();
    let snapshot_a = runtime
        .prepare(
            "thread-a",
            &agent,
            cwd.clone(),
            None,
            false,
            limits,
            events(),
        )
        .await
        .expect("prepare thread-a");
    runtime
        .prepare(
            "thread-b",
            &agent,
            cwd.clone(),
            None,
            false,
            limits,
            events_b,
        )
        .await
        .expect("prepare thread-b");
    while received_b.try_recv().is_ok() {}
    let handle = runtime
        .schedule_prompt(
            "thread-a",
            &agent,
            cwd,
            prompt("permission"),
            Some(snapshot_a.session_id.clone()),
            false,
            limits,
            events_a,
        )
        .await
        .expect("schedule permission prompt");

    let request_id = tokio::time::timeout(Duration::from_secs(2), async {
        let mut saw_tool_call = false;
        loop {
            match received_a.recv().await {
                Some(AcpEvent::ToolCall {
                    tool_call_id,
                    title,
                    ..
                }) => {
                    assert_eq!(tool_call_id, format!("tool-{}", snapshot_a.session_id));
                    assert_eq!(title.as_deref(), Some("Edit file"));
                    saw_tool_call = true;
                }
                Some(AcpEvent::PermissionRequest {
                    request_id,
                    options,
                    ..
                }) => {
                    assert!(
                        saw_tool_call,
                        "tool row must precede its permission interaction"
                    );
                    assert_eq!(
                        options
                            .iter()
                            .map(|option| option.option_id.as_str())
                            .collect::<Vec<_>>(),
                        ["allow-once", "reject-once"]
                    );
                    break request_id;
                }
                Some(_) => {}
                None => panic!("thread-a ACP event stream closed before permission"),
            }
        }
    })
    .await
    .expect("thread-a permission request");
    assert!(std::iter::from_fn(|| received_b.try_recv().ok())
        .all(|event| !matches!(event, AcpEvent::PermissionRequest { .. })));
    assert!(
        runtime
            .resolve_permission(&request_id, "allow-once".into(), None)
            .await
    );
    handle.wait().await.expect("permission prompt completes");

    let log = std::fs::read_to_string(&log_path).expect("fake agent log");
    let response = log
        .lines()
        .find(|line| line.starts_with("permission/response\t"))
        .expect("permission response log");
    assert!(response.contains(&snapshot_a.session_id), "{log}");
    assert!(response.contains("allow-once"), "{log}");

    std::fs::remove_file(log_path).expect("remove fake agent log");
}

#[tokio::test]
async fn codex_form_elicitation_is_exposed_as_question_and_returns_typed_content() {
    let log_path = unique_log_path("codex-form-elicitation");
    let runtime = AcpRuntime::new();
    let agent = fake_agent(&log_path);
    let limits = RuntimeLimits::new(60, 8);
    let cwd = std::env::current_dir().expect("current directory");
    let (event_tx, mut received) = mpsc::unbounded_channel();
    let snapshot = runtime
        .prepare(
            "thread-codex-form",
            &agent,
            cwd.clone(),
            None,
            false,
            limits,
            events(),
        )
        .await
        .expect("prepare Codex form session");
    let handle = runtime
        .schedule_prompt(
            "thread-codex-form",
            &agent,
            cwd,
            prompt("codex-form-elicitation"),
            Some(snapshot.session_id.clone()),
            false,
            limits,
            event_tx,
        )
        .await
        .expect("schedule Codex form prompt");

    let expected_tool_call_id = format!("question-{}", snapshot.session_id);
    let request_id = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            match received.recv().await {
                Some(AcpEvent::PermissionRequest {
                    request_id,
                    interaction_kind,
                    tool_call_id,
                    raw,
                    ..
                }) => {
                    assert_eq!(interaction_kind, AcpInteractionKind::Question);
                    assert_eq!(
                        tool_call_id.as_deref(),
                        Some(expected_tool_call_id.as_str())
                    );
                    assert_eq!(raw["kind"], "elicitation_form");
                    assert_eq!(raw["questions"][0]["id"], "scope");
                    assert_eq!(raw["questions"][0]["options"][0]["value"], "toolbar");
                    assert_eq!(raw["questions"][0]["allowOther"], true);
                    assert_eq!(raw["questions"][1]["id"], "variant_count");
                    assert_eq!(raw["questions"][1]["inputType"], "integer");
                    break request_id;
                }
                Some(_) => {}
                None => panic!("ACP event stream closed before Codex form elicitation"),
            }
        }
    })
    .await
    .expect("Codex form elicitation must reach the question UI");

    runtime
        .resolve_questionnaire(
            &request_id,
            AcpQuestionnaireSubmission {
                outcome: AcpQuestionnaireOutcome::Accepted,
                answers: vec![
                    AcpQuestionnaireAnswer {
                        question_index: 0,
                        selected_option_indexes: vec![0],
                        other_text: None,
                    },
                    AcpQuestionnaireAnswer {
                        question_index: 1,
                        selected_option_indexes: Vec::new(),
                        other_text: Some("2".into()),
                    },
                ],
            },
        )
        .await
        .expect("resolve Codex form elicitation");
    handle.wait().await.expect("Codex form prompt completes");

    let log = std::fs::read_to_string(&log_path).expect("fake agent log");
    let response = log
        .lines()
        .find(|line| line.starts_with("elicitation/response\t"))
        .expect("elicitation response log");
    assert!(response.contains(r#""action": "accept""#), "{log}");
    assert!(response.contains(r#""scope": "toolbar""#), "{log}");
    assert!(response.contains(r#""variant_count": 2"#), "{log}");

    std::fs::remove_file(log_path).expect("remove fake agent log");
}

#[tokio::test]
async fn codex_plan_review_is_never_auto_approved_and_uses_plan_review_interaction() {
    let log_path = unique_log_path("codex-plan-review");
    let runtime = AcpRuntime::new();
    let agent = fake_agent(&log_path);
    let limits = RuntimeLimits::new(60, 8);
    let cwd = std::env::current_dir().expect("current directory");
    let (event_tx, mut received) = mpsc::unbounded_channel();
    let snapshot = runtime
        .prepare(
            "thread-codex-plan",
            &agent,
            cwd.clone(),
            None,
            true,
            limits,
            events(),
        )
        .await
        .expect("prepare Codex plan session");
    let handle = runtime
        .schedule_prompt(
            "thread-codex-plan",
            &agent,
            cwd,
            prompt("codex-plan-review"),
            Some(snapshot.session_id.clone()),
            true,
            limits,
            event_tx,
        )
        .await
        .expect("schedule Codex plan prompt");

    let request_id = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            match received.recv().await {
                Some(AcpEvent::PermissionRequest {
                    request_id,
                    interaction_kind,
                    raw,
                    options,
                    ..
                }) => {
                    assert_eq!(interaction_kind, AcpInteractionKind::PlanReview);
                    assert_eq!(raw["kind"], "plan_review");
                    assert_eq!(raw["plan"], "# Test Plan\n\n- Step one");
                    assert_eq!(raw["supportsFeedback"], true);
                    assert_eq!(raw["feedbackDelivery"], "follow_up_prompt");
                    assert_eq!(
                        options
                            .iter()
                            .map(|option| option.option_id.as_str())
                            .collect::<Vec<_>>(),
                        ["implement_plan", "revise_plan"]
                    );
                    break request_id;
                }
                Some(AcpEvent::ToolCall { tool_call_id, .. })
                    if tool_call_id == format!("plan-{}", snapshot.session_id) =>
                {
                    panic!("plan review must not create a duplicate generic tool row");
                }
                Some(_) => {}
                None => panic!("ACP event stream closed before Codex plan review"),
            }
        }
    })
    .await
    .expect("Codex plan review must not be consumed by auto approval");

    assert!(
        runtime
            .resolve_permission(&request_id, "implement_plan".into(), None)
            .await
    );
    handle
        .wait()
        .await
        .expect("Codex plan review prompt completes");

    let log = std::fs::read_to_string(&log_path).expect("fake agent log");
    let response = log
        .lines()
        .find(|line| line.starts_with("plan-review/response\t"))
        .expect("plan review response log");
    assert!(response.contains("implement_plan"), "{log}");

    std::fs::remove_file(log_path).expect("remove fake agent log");
}

#[tokio::test]
async fn codex_plan_review_can_be_cancelled_without_selecting_an_unknown_option() {
    let log_path = unique_log_path("codex-plan-cancel");
    let runtime = AcpRuntime::new();
    let agent = fake_agent(&log_path);
    let limits = RuntimeLimits::new(60, 8);
    let cwd = std::env::current_dir().expect("current directory");
    let (event_tx, mut received) = mpsc::unbounded_channel();
    let snapshot = runtime
        .prepare(
            "thread-codex-plan-cancel",
            &agent,
            cwd.clone(),
            None,
            false,
            limits,
            events(),
        )
        .await
        .expect("prepare Codex plan session");
    let handle = runtime
        .schedule_prompt(
            "thread-codex-plan-cancel",
            &agent,
            cwd,
            prompt("codex-plan-review"),
            Some(snapshot.session_id),
            false,
            limits,
            event_tx,
        )
        .await
        .expect("schedule Codex plan prompt");

    let request_id = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if let Some(AcpEvent::PermissionRequest { request_id, .. }) = received.recv().await {
                break request_id;
            }
        }
    })
    .await
    .expect("receive Codex plan review");

    assert!(runtime.cancel_interaction(&request_id).await);
    let (outcome, selected_option_id) = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if let Some(AcpEvent::InteractionClosed {
                outcome,
                selected_option_id,
                ..
            }) = received.recv().await
            {
                break (outcome, selected_option_id);
            }
        }
    })
    .await
    .expect("receive cancelled plan terminal event");
    assert_eq!(outcome, AcpInteractionOutcome::Cancelled);
    assert_eq!(selected_option_id, None);
    handle
        .wait()
        .await
        .expect("cancelled review completes prompt");

    let log = std::fs::read_to_string(&log_path).expect("fake agent log");
    let response = log
        .lines()
        .find(|line| line.starts_with("plan-review/response\t"))
        .expect("plan review response log");
    assert!(response.contains(r#""outcome": "cancelled""#), "{log}");
    assert!(!response.contains("abandoned"), "{log}");

    std::fs::remove_file(log_path).expect("remove fake agent log");
}

#[tokio::test]
async fn qwen_question_extension_bypasses_auto_approval_and_returns_answers() {
    let log_path = unique_log_path("qwen-user-question");
    let runtime = AcpRuntime::new();
    let agent = fake_agent(&log_path);
    let limits = RuntimeLimits::new(60, 8);
    let cwd = std::env::current_dir().expect("current directory");
    let (event_tx, mut received) = mpsc::unbounded_channel();
    let snapshot = runtime
        .prepare(
            "thread-qwen-question",
            &agent,
            cwd.clone(),
            None,
            true,
            limits,
            events(),
        )
        .await
        .expect("prepare Qwen question session");
    let handle = runtime
        .schedule_prompt(
            "thread-qwen-question",
            &agent,
            cwd,
            prompt("qwen-user-question"),
            Some(snapshot.session_id.clone()),
            true,
            limits,
            event_tx,
        )
        .await
        .expect("schedule Qwen question prompt");

    let expected_tool_call_id = format!("qwen-question-{}", snapshot.session_id);
    let request_id = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            match received.recv().await {
                Some(AcpEvent::PermissionRequest {
                    request_id,
                    interaction_kind,
                    tool_call_id,
                    raw,
                    ..
                }) => {
                    assert_eq!(interaction_kind, AcpInteractionKind::Question);
                    assert_eq!(
                        tool_call_id.as_deref(),
                        Some(expected_tool_call_id.as_str())
                    );
                    assert_eq!(raw["kind"], "ask_user_question");
                    assert_eq!(
                        raw["questions"][0]["question"],
                        "Which language should the project use?"
                    );
                    assert_eq!(raw["questions"][0]["allowOther"], true);
                    assert_eq!(raw["questions"][1]["multiSelect"], true);
                    break request_id;
                }
                Some(AcpEvent::ToolCall { tool_call_id, .. })
                    if tool_call_id == expected_tool_call_id =>
                {
                    panic!("question interaction must not create a duplicate generic tool row");
                }
                Some(_) => {}
                None => panic!("ACP event stream closed before Qwen question"),
            }
        }
    })
    .await
    .expect("Qwen question must not be consumed by auto approval");

    runtime
        .resolve_questionnaire(
            &request_id,
            AcpQuestionnaireSubmission {
                outcome: AcpQuestionnaireOutcome::Accepted,
                answers: vec![
                    AcpQuestionnaireAnswer {
                        question_index: 0,
                        selected_option_indexes: vec![0],
                        other_text: None,
                    },
                    AcpQuestionnaireAnswer {
                        question_index: 1,
                        selected_option_indexes: vec![0],
                        other_text: Some("Security scan".into()),
                    },
                ],
            },
        )
        .await
        .expect("resolve Qwen questionnaire");
    handle.wait().await.expect("Qwen question prompt completes");

    let log = std::fs::read_to_string(&log_path).expect("fake agent log");
    let response = log
        .lines()
        .find(|line| line.starts_with("qwen-question/response\t"))
        .expect("Qwen question response log");
    assert!(response.contains(r#""optionId": "proceed_once""#), "{log}");
    assert!(response.contains(r#""0": "TypeScript""#), "{log}");
    assert!(
        response.contains(r#""1": "Unit tests, Security scan""#),
        "{log}"
    );

    std::fs::remove_file(log_path).expect("remove fake agent log");
}

#[tokio::test]
async fn claude_switch_mode_with_plan_is_classified_without_vendor_metadata() {
    let log_path = unique_log_path("claude-plan-review");
    let runtime = AcpRuntime::new();
    let agent = fake_agent(&log_path);
    let limits = RuntimeLimits::new(60, 8);
    let cwd = std::env::current_dir().expect("current directory");
    let (event_tx, mut received) = mpsc::unbounded_channel();
    let snapshot = runtime
        .prepare(
            "thread-claude-plan",
            &agent,
            cwd.clone(),
            None,
            true,
            limits,
            events(),
        )
        .await
        .expect("prepare Claude plan session");
    let handle = runtime
        .schedule_prompt(
            "thread-claude-plan",
            &agent,
            cwd,
            prompt("claude-plan-review"),
            Some(snapshot.session_id.clone()),
            true,
            limits,
            event_tx,
        )
        .await
        .expect("schedule Claude plan prompt");

    let expected_tool_call_id = format!("claude-plan-{}", snapshot.session_id);
    let request_id = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            match received.recv().await {
                Some(AcpEvent::PermissionRequest {
                    request_id,
                    interaction_kind,
                    tool_call_id,
                    raw,
                    options,
                    ..
                }) => {
                    assert_eq!(interaction_kind, AcpInteractionKind::PlanReview);
                    assert_eq!(
                        tool_call_id.as_deref(),
                        Some(expected_tool_call_id.as_str())
                    );
                    assert_eq!(raw["kind"], "plan_review");
                    assert_eq!(raw["plan"], "# Claude Plan\n\n- Keep the API stable");
                    assert_eq!(
                        options
                            .iter()
                            .map(|option| option.option_id.as_str())
                            .collect::<Vec<_>>(),
                        ["acceptEdits", "default", "plan"]
                    );
                    break request_id;
                }
                Some(AcpEvent::ToolCall { tool_call_id, .. })
                    if tool_call_id == expected_tool_call_id =>
                {
                    panic!("Claude plan review must not create a duplicate generic tool row");
                }
                Some(_) => {}
                None => panic!("ACP event stream closed before Claude plan review"),
            }
        }
    })
    .await
    .expect("Claude switch-mode request must become a plan review");

    assert!(
        runtime
            .resolve_permission(&request_id, "default".into(), None)
            .await
    );
    handle.wait().await.expect("Claude plan prompt completes");

    let log = std::fs::read_to_string(&log_path).expect("fake agent log");
    let response = log
        .lines()
        .find(|line| line.starts_with("claude-plan/response\t"))
        .expect("Claude plan response log");
    assert!(response.contains(r#""optionId": "default""#), "{log}");

    std::fs::remove_file(log_path).expect("remove fake agent log");
}
