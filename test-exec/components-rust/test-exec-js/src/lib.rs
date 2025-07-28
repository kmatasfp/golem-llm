#[allow(static_mut_refs)]
mod bindings;

use crate::bindings::exports::test::exec_js_exports::test_exec_js_api::*;
use crate::bindings::golem::exec::executor::run;
use crate::bindings::golem::exec::types::{Encoding, File, Language, LanguageKind};
use indoc::indoc;

struct Component;

impl Guest for Component {
    fn test1() -> bool {
        match run(
            &Language {
                kind: LanguageKind::Javascript,
                version: None,
            },
            indoc!(
                r#"
            const x = 40 + 2;
            const name = "world";
            console.log(`Hello, ${name}!`, x);
            "#
            ),
            &[],
            None,
            &[],
            &[],
            None,
        ) {
            Ok(result) => {
                println!("Result: {:?}", result);
                result.run.stdout == "Hello, world! 42" && result.run.exit_code == Some(0)
            }
            Err(err) => {
                println!("Error: {}", err);
                false
            }
        }
    }

    fn test2() -> bool {
        match run(
            &Language {
                kind: LanguageKind::Javascript,
                version: None,
            },
            indoc!(
                r#"
                import { createInterface } from "node:readline";

                const rl = createInterface({
                    input: process.stdin,
                    output: process.stdout
                });

                let sum = 0;

                rl.on('line', (line) => {
                    const number = parseFloat(line);
                    if (!isNaN(number)) {
                        sum += number;
                    }
                });

                rl.on('close', () => {
                    console.log(`Total Sum: ${sum}`);
                });
            "#
            ),
            &[],
            Some("1\n2\n3\n"),
            &[],
            &[],
            None,
        ) {
            Ok(result) => {
                println!("Result: {:?}", result);
                result.run.stdout == "Total Sum: 6" && result.run.exit_code == Some(0)
            }
            Err(err) => {
                println!("Error: {}", err);
                false
            }
        }
    }

    fn test3() -> bool {
        match run(
            &Language {
                kind: LanguageKind::Javascript,
                version: None,
            },
            indoc!(
                r#"
                import { createInterface } from "node:readline";

                const rl = createInterface({
                    input: process.stdin,
                    output: process.stdout
                });

                let sum = 0;

                async function calculateSum() {
                    for await (const line of rl) {
                        const number = parseFloat(line);
                        if (!isNaN(number)) {
                            sum += number;
                        }
                    }
                    console.log(`Total Sum: ${sum}`);
                }

                await calculateSum();
            "#
            ),
            &[],
            Some("1\n2\n3\n"),
            &[],
            &[],
            None,
        ) {
            Ok(result) => {
                println!("Result: {:?}", result);
                result.run.stdout == "Total Sum: 6" && result.run.exit_code == Some(0)
            }
            Err(err) => {
                println!("Error: {}", err);
                false
            }
        }
    }

    fn test4() -> bool {
        match run(
            &Language {
                kind: LanguageKind::Javascript,
                version: None,
            },
            indoc!(
                r#"
                import { argv } from "node:process";
                console.log(...argv);
                "#
            ),
            &[],
            None,
            &["arg1".to_string(), "arg2".to_string()],
            &[],
            None,
        ) {
            Ok(result) => {
                println!("Result: {:?}", result);
                result.run.stdout == "arg1 arg2" && result.run.exit_code == Some(0)
            }
            Err(err) => {
                println!("Error: {}", err);
                false
            }
        }
    }

    fn test5() -> bool {
        match run(
            &Language {
                kind: LanguageKind::Javascript,
                version: None,
            },
            indoc!(
                r#"
                import { env } from "node:process";
                console.log(env.INPUT);
                "#
            ),
            &[],
            None,
            &[],
            &[("INPUT".to_string(), "test_value".to_string())],
            None,
        ) {
            Ok(result) => {
                println!("Result: {:?}", result);
                result.run.stdout == "test_value" && result.run.exit_code == Some(0)
            }
            Err(err) => {
                println!("Error: {}", err);
                false
            }
        }
    }

    fn test6() -> bool {
        match run(
            &Language {
                kind: LanguageKind::Javascript,
                version: None,
            },
            indoc!(
                r#"
                import { x, name } from "test/module.js";
                console.log(`Hello, ${name}!`, x);
                "#
            ),
            &[File {
                name: "test/module.js".to_string(),
                content: indoc!(
                    r#"
                        export const x = 40 + 2;
                        export const name = "world";
                        "#
                )
                .as_bytes()
                .to_vec(),
                encoding: Some(Encoding::Utf8),
            }],
            None,
            &[],
            &[],
            None,
        ) {
            Ok(result) => {
                println!("Result: {:?}", result);
                result.run.stdout == "Hello, world! 42" && result.run.exit_code == Some(0)
            }
            Err(err) => {
                println!("Error: {}", err);
                false
            }
        }
    }

    fn test7() -> bool {
        let session = bindings::golem::exec::executor::Session::new(
            &Language {
                kind: LanguageKind::Javascript,
                version: None,
            },
            &[File {
                name: "test/module.js".to_string(),
                content: indoc!(
                    r#"
                        export const x = 40 + 2;
                        export const name = "world";
                        "#
                )
                .as_bytes()
                .to_vec(),
                encoding: Some(Encoding::Utf8),
            }],
        );

        let r1 = session
            .run(
                indoc!(
                    r#"
                import { x, name } from "test/module.js";
                console.log(`Hello, ${name}!`, x);
                "#
                ),
                &[],
                None,
                &[],
                None,
            )
            .map_or_else(
                |err| {
                    println!("Error: {}", err);
                    false
                },
                |result| {
                    println!("Result: {:?}", result);
                    result.run.stdout == "Hello, world! 42" && result.run.exit_code == Some(0)
                },
            );

        let r2 = session
            .run(
                indoc!(
                    r#"
                import { argv } from "node:process";
                console.log(...argv);
                "#
                ),
                &["arg1".to_string(), "arg2".to_string()],
                None,
                &[],
                None,
            )
            .map_or_else(
                |err| {
                    println!("Error: {}", err);
                    false
                },
                |result| {
                    println!("Result: {:?}", result);
                    result.run.stdout == "arg1 arg2" && result.run.exit_code == Some(0)
                },
            );

        let r3 = session
            .run(
                indoc!(
                    r#"
                import { argv } from "node:process";
                console.log(...argv);
                "#
                ),
                &["arg3".to_string()],
                None,
                &[],
                None,
            )
            .map_or_else(
                |err| {
                    println!("Error: {}", err);
                    false
                },
                |result| {
                    println!("Result: {:?}", result);
                    result.run.stdout == "arg3" && result.run.exit_code == Some(0)
                },
            );

        const READLINE_SNIPPET: &str = indoc!(
            r#"
                import { createInterface } from "node:readline";

                const rl = createInterface({
                    input: process.stdin,
                    output: process.stdout
                });

                let sum = 0;

                async function calculateSum() {
                    for await (const line of rl) {
                        const number = parseFloat(line);
                        if (!isNaN(number)) {
                            sum += number;
                        }
                    }
                    console.log(`Total Sum: ${sum}`);
                }

                await calculateSum();
            "#
        );

        let r4 = session
            .run(READLINE_SNIPPET, &[], Some("1\n2\n3\n"), &[], None)
            .map_or_else(
                |err| {
                    println!("Error: {}", err);
                    false
                },
                |result| {
                    println!("Result: {:?}", result);
                    result.run.stdout == "Total Sum: 6" && result.run.exit_code == Some(0)
                },
            );
        let r5 = session
            .run(READLINE_SNIPPET, &[], Some("4\n100\n"), &[], None)
            .map_or_else(
                |err| {
                    println!("Error: {}", err);
                    false
                },
                |result| {
                    println!("Result: {:?}", result);
                    result.run.stdout == "Total Sum: 104" && result.run.exit_code == Some(0)
                },
            );

        r1 && r2 && r3 && r4 && r5
    }

    fn test8() -> bool {
        let session = bindings::golem::exec::executor::Session::new(
            &Language {
                kind: LanguageKind::Javascript,
                version: None,
            },
            &[],
        );

        let r1 = session
            .upload(&File {
                name: "test/input.txt".to_string(),
                content: "Hello, Golem!".as_bytes().to_vec(),
                encoding: Some(Encoding::Utf8),
            })
            .map_or_else(
                |err| {
                    println!("Error uploading file: {}", err);
                    false
                },
                |_| true,
            );

        let r2 = session
            .run(
                indoc!(
                    r#"
                import { readFileSync, writeFileSync } from "node:fs";
                const content = readFileSync("test/input.txt", "utf8");
                console.log(content);
                writeFileSync("test/output.txt", content + " - Processed by Golem");
                "#
                ),
                &[],
                None,
                &[],
                None,
            )
            .map_or_else(
                |err| {
                    println!("Error running script: {}", err);
                    false
                },
                |result| {
                    println!("Result: {:?}", result);
                    result.run.stdout == "Hello, Golem!" && result.run.exit_code == Some(0)
                },
            );

        let r3 = session.download("test/output.txt").map_or_else(
            |err| {
                println!("Error downloading file: {}", err);
                false
            },
            |file| {
                let content = String::from_utf8(file).unwrap_or_default();
                println!("Downloaded file content: {}", content);
                content == "Hello, Golem! - Processed by Golem"
            },
        );

        r1 && r2 && r3
    }

    fn test9() -> bool {
        let session = bindings::golem::exec::executor::Session::new(
            &Language {
                kind: LanguageKind::Javascript,
                version: None,
            },
            &[],
        );

        let r1 = session
            .upload(&File {
                name: "test/input.txt".to_string(),
                content: "Hello, Golem!".as_bytes().to_vec(),
                encoding: Some(Encoding::Utf8),
            })
            .map_or_else(
                |err| {
                    println!("Error uploading file: {}", err);
                    false
                },
                |_| true,
            );

        let r2 = session
            .run(
                indoc!(
                    r#"
                import { readFile, writeFile } from "node:fs";
                readFile("test/input.txt", "utf8", (content, error) => {
                    if (error) {
                        console.error("Error reading file:", error);
                        return;
                    }
                    console.log(content);
                    writeFile("test/output.txt", content + " - Processed by Golem", (error) => {
                        if (error) {
                            console.error("Error writing file:", error);
                            return;
                        }
                    });
                });
                "#
                ),
                &[],
                None,
                &[],
                None,
            )
            .map_or_else(
                |err| {
                    println!("Error running script: {}", err);
                    false
                },
                |result| {
                    println!("Result: {:?}", result);
                    result.run.stdout == "Hello, Golem!" && result.run.exit_code == Some(0)
                },
            );

        let r3 = session.download("test/output.txt").map_or_else(
            |err| {
                println!("Error downloading file: {}", err);
                false
            },
            |file| {
                let content = String::from_utf8(file).unwrap_or_default();
                println!("Downloaded file content: {}", content);
                content == "Hello, Golem! - Processed by Golem"
            },
        );

        let r4 = session.set_working_dir("test").map_or_else(
            |err| {
                println!("Error setting working directory: {}", err);
                false
            },
            |_| true,
        );

        let r5 = session
            .run(
                indoc!(
                    r#"
                    import { readFile, writeFile } from "node:fs";
                    import { cwd } from "node:process";

                    console.log("Current working directory:", cwd());
                    readFile("input.txt", "utf8", (content, error) => {
                        if (error) {
                            console.error("Error reading file:", error);
                            return;
                        }
                        console.log(content);
                        writeFile("/test/output2.txt", content + " - Processed by Golem", (error) => {
                            if (error) {
                                console.error("Error writing file:", error);
                                return;
                            }
                        });
                    });
                "#
                ),
                &[],
                None,
                &[],
                None,
            )
            .map_or_else(
                |err| {
                    println!("Error running script: {}", err);
                    false
                },
                |result| {
                    println!("Result: {:?}", result);
                    result.run.stdout == "Current working directory: test\nHello, Golem!" && result.run.exit_code == Some(0)
                },
            );

        let r6 = session.download("test/output2.txt").map_or_else(
            |err| {
                println!("Error downloading file: {}", err);
                false
            },
            |file| {
                let content = String::from_utf8(file).unwrap_or_default();
                println!("Downloaded file content: {}", content);
                content == "Hello, Golem! - Processed by Golem"
            },
        );

        r1 && r2 && r3 && r4 && r5 && r6
    }
}

bindings::export!(Component with_types_in bindings);
