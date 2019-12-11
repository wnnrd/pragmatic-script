use crate::{
    parser::{
        parser::*,
        ast::*,
        lexer::*
    }
};

use logos::Logos;

#[test]
fn test_neg_parse_struct_decl() {
    let code = String::from("
        struct: Integer {
            inner: int;
            inner: int;
        }
    ");

    let mut lexer = Token::lexer(code.as_str());
    let parser = Parser::new(code.clone());

    let decl_res = parser.parse_struct_decl(&mut lexer);
    assert!(decl_res.is_err());
}

#[test]
fn test_parse_struct_decl() {
    let code = String::from("
        struct: Integer {
            inner: int;
        }
    ");

    let mut lexer = Token::lexer(code.as_str());
    let parser = Parser::new(code.clone());

    let decl_res = parser.parse_struct_decl(&mut lexer);
    assert!(decl_res.is_ok());
}

#[test]
fn test_parse_empty_fn_decl() {
    let code = String::from("fn: main(arg: int) ~ int;");
    let mut lexer = Token::lexer(code.as_str());
    let parser = Parser::new(code.clone());
    let decl_res = parser.parse_fn_decl(&mut lexer);

    assert!(decl_res.is_ok());

    if let Declaration::Function(fn_decl) = decl_res.unwrap() {
        assert_eq!(fn_decl.name, String::from("main"));
    assert_eq!(fn_decl.arguments.len(), 1);
    assert!(fn_decl.code_block.is_none());
    }
}

#[test]
fn test_parse_full_fn_decl() {
    let code = String::from("fn: main(arg: int) ~ int {}");
    let mut lexer = Token::lexer(code.as_str());
    let parser = Parser::new(code.clone());
    let decl_res = parser.parse_fn_decl(&mut lexer);

    assert!(decl_res.is_ok());

    if let Declaration::Function(fn_decl) = decl_res.unwrap() {
        assert_eq!(fn_decl.name, String::from("main"));
        assert_eq!(fn_decl.arguments.len(), 1);
        assert!(fn_decl.code_block.is_some());
    }
}

#[test]
fn test_parse_fn_mul_args() {
    let code = String::from("fn: main21(arg: int, noarg: int) ~ int {}");
    let mut lexer = Token::lexer(code.as_str());
    let parser = Parser::new(code.clone());
    let decl_res = parser.parse_fn_decl(&mut lexer);

    assert!(decl_res.is_ok());

    if let Declaration::Function(fn_decl) = decl_res.unwrap() {
        assert_eq!(fn_decl.name, String::from("main21"));
        assert_eq!(fn_decl.arguments.len(), 2);
        assert!(fn_decl.code_block.is_some());
    }
}

#[test]
fn test_parse_decl_list() {
    let code = String::from("
        fn: main1(argc: int) ~ int;
        fn: test2(noint: float) ~ float {}
    ");
    let parser = Parser::new(code);

    let decl_list_res = parser.parse_decl_list();

    assert!(decl_list_res.is_ok());

    let decl_list = decl_list_res.unwrap();

    assert_eq!(decl_list.len(), 2);
}

#[test]
fn test_parse_stmt_list() {
    let code = String::from("
        var:int x = 4;
        var:int y = 6;
    ");

    let mut lexer = Token::lexer(code.as_str());
    let parser = Parser::new(code.clone());
    let stmt_list_res = parser.parse_statement_list(&mut lexer);

    assert!(stmt_list_res.is_ok());
    let stmt_list = stmt_list_res.unwrap();

    assert_eq!(stmt_list.len(), 2);
}

#[test]
fn test_parse_stmt_addition() {
    let code = String::from("
        var:int x = 4;
        y = 1 + 2 * 3 + x;
    ");

    let mut lexer = Token::lexer(code.as_str());
    let parser = Parser::new(code.clone());
    let stmt_list_res = parser.parse_statement_list(&mut lexer);

    assert!(stmt_list_res.is_ok());
    let stmt_list = stmt_list_res.unwrap();

    assert_eq!(stmt_list.len(), 2);

    println!("{:?}", stmt_list);
}

#[test]
fn test_parse_raw_expr() {
    let code = String::from("
        (1 + 2 + 3) * 7 - 8 + 3;
    ");
    let mut lexer = Token::lexer(code.as_str());
    let parser = Parser::new(code.clone());

    let expr_res = parser.parse_expr(&mut lexer, &[Token::Semicolon]);
    assert!(expr_res.is_ok());
    let expr = expr_res.unwrap();
    expr.print(0);
}

#[test]
fn test_parse_raw_var_expr() {
    let code = String::from("
        (1 + z + 3) * x - 8 + y;
    ");
    let mut lexer = Token::lexer(code.as_str());
    let parser = Parser::new(code.clone());
    let expr_res = parser.parse_expr(&mut lexer, &[Token::Semicolon]);
    assert!(expr_res.is_ok());
    let expr = expr_res.unwrap();
    //expr.print(0);
}

#[test]
fn test_parse_full_fn() {
    let code = String::from("
        fn: main(argc: int) ~ int {
            var:int x = 4;
            var:int y = 6;
            return x + y;
        }
    ");

    let parser = Parser::new(code.clone());
    let decl_list_res = parser.parse_decl_list();
    assert!(decl_list_res.is_ok());
}

#[test]
fn test_parse_expr_paran_delim() {
    use crate::{
        parser::ast::*  
    };

    let code = String::from("
        (1 + 2) + 2)
    ");
    let mut lexer = Token::lexer(code.as_str());
    let parser = Parser::new(code.clone());
    let expr_res = parser.parse_expr(&mut lexer, &[
        Token::CloseParan
    ]);
    assert!(expr_res.is_ok());
    let expr = expr_res.unwrap();
    match expr {
        Expression::Addition(lhs, rhs) => {
            match *lhs {
                Expression::Addition(lhs, rhs) => {
                    match *lhs {
                        Expression::IntLiteral(_) => {},
                        _ => {
                            panic!("Incorrect expression! Should be IntLiteral.");
                        }
                    };
                    match *rhs {
                        Expression::IntLiteral(_) => {},
                        _ => {
                            panic!("Incorrect expression! Should be IntLiteral.");
                        }
                    };
                },
                _ => {
                    panic!("Incorrect expression! Should be Addition.");
                }
            };
            match *rhs {
                Expression::IntLiteral(_) => {},
                _ => {
                    panic!("Incorrect expression! Should be IntLiteral.");
                }
            };
        },
        _ => {
            panic!("Incorrect expression! Should be Addition.");
        }
    }
}

#[test]
fn test_parse_call_stmt() {
    use crate::{
        parser::ast::*  
    };

    let code = String::from("
        add(5, 5);
    ");
    let mut lexer = Token::lexer(code.as_str());
    let parser = Parser::new(code.clone());
    let stmt_res = parser.parse_fn_call_stmt(&mut lexer);
    assert!(stmt_res.is_ok());
    if let Statement::Call(name, args) = stmt_res.unwrap() {
        assert_eq!(name, String::from("add"));
        assert_eq!(args.len(), 2);
        assert_eq!(args, vec![
            Expression::IntLiteral(5),
            Expression::IntLiteral(5)
        ]);
    }
}

#[test]
fn test_parse_call_expr() {
    use crate::parser::ast::Expression;

    let code = String::from("
        add(5, 5)
    ");
    let mut lexer = Token::lexer(code.as_str());
    let parser = Parser::new(code.clone());
    let expr_res = parser.try_parse_call_expr(&mut lexer);
    assert!(expr_res.is_ok());
    if let Expression::Call(name, args) = expr_res.unwrap() {
        assert_eq!(name, String::from("add"));
        assert_eq!(args.len(), 2);
        assert_eq!(args, vec![
            Expression::IntLiteral(5),
            Expression::IntLiteral(5)
        ]);
    }
}

#[test]
fn test_parse_complex_call_expr() {
    use crate::parser::ast::Expression;

    let code = String::from("
        add(5, 5) + 5;
    ");
    let mut lexer = Token::lexer(code.as_str());
    let parser = Parser::new(code.clone());
    let expr_res = parser.parse_expr(&mut lexer, &[
        Token::Semicolon
    ]);
    assert!(expr_res.is_ok());
    let expr = expr_res.unwrap();
    match expr {
        Expression::Addition(lhs, rhs) => {
            match *lhs {
                Expression::Call(fn_name, args) => {
                    assert_eq!(fn_name, String::from("add"));
                    assert_eq!(args.len(), 2);
                },
                _ => {
                    panic!("Wrong expression! Should be Call.");
                }
            };
            match *rhs {
                Expression::IntLiteral(int) => {
                    assert_eq!(int, 5);
                },
                _ => {
                    panic!("Wrong expression! Should be IntLiteral.");
                }
            };
        },
        _ => {
            panic!("Wrong expression! Should be Addition.");
        }
    }
}