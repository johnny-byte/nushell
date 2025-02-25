use nu_test_support::fs::Stub::FileWithContentToBeTrimmed;
use nu_test_support::playground::Playground;
use nu_test_support::{nu, pipeline};

#[test]
fn changes_the_column_name() {
    Playground::setup("rename_test_1", |dirs, sandbox| {
        sandbox.with_files(vec![FileWithContentToBeTrimmed(
            "los_cuatro_mosqueteros.txt",
            r#"
                Andrés N. Robalino
                Jonathan Turner
                Yehuda Katz
                Jason Gedge
            "#,
        )]);

        let actual = nu!(
            cwd: dirs.test(), pipeline(
            r#"
                open los_cuatro_mosqueteros.txt
                | lines
                | wrap name
                | rename mosqueteros
                | get mosqueteros
                | length
                "#
        ));

        assert_eq!(actual.out, "4");
    })
}

#[test]
fn keeps_remaining_original_names_given_less_new_names_than_total_original_names() {
    Playground::setup("rename_test_2", |dirs, sandbox| {
        sandbox.with_files(vec![FileWithContentToBeTrimmed(
            "los_cuatro_mosqueteros.txt",
            r#"
                Andrés N. Robalino
                Jonathan Turner
                Yehuda Katz
                Jason Gedge
            "#,
        )]);

        let actual = nu!(
            cwd: dirs.test(), pipeline(
            r#"
                open los_cuatro_mosqueteros.txt
                | lines
                | wrap name
                | default "arepa!" hit
                | rename mosqueteros
                | get hit
                | length
                "#
        ));

        assert_eq!(actual.out, "4");
    })
}

#[test]
fn errors_if_no_columns_present() {
    Playground::setup("rename_test_3", |dirs, sandbox| {
        sandbox.with_files(vec![FileWithContentToBeTrimmed(
            "los_cuatro_mosqueteros.txt",
            r#"
                Andrés N. Robalino
                Jonathan Turner
                Yehuda Katz
                Jason Gedge
            "#,
        )]);

        let actual = nu!(
            cwd: dirs.test(), pipeline(
            r#"
                open los_cuatro_mosqueteros.txt
                | lines
                | rename mosqueteros
                "#
        ));

        assert!(actual.err.contains("only record input data is supported"));
    })
}

#[test]
fn errors_if_columns_param_is_empty() {
    Playground::setup("rename_test_4", |dirs, sandbox| {
        sandbox.with_files(vec![FileWithContentToBeTrimmed(
            "los_cuatro_mosqueteros.txt",
            r#"
                Andrés N. Robalino
                Jonathan Turner
                Yehuda Katz
                Jason Gedge
            "#,
        )]);

        let actual = nu!(
            cwd: dirs.test(), pipeline(
            r#"
                open los_cuatro_mosqueteros.txt
                | lines
                | wrap name
                | default "arepa!" hit
                | rename -c []
                "#
        ));

        assert!(actual.err.contains("The column list cannot be empty"));
    })
}
