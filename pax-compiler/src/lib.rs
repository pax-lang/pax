extern crate core;

pub mod manifest;
pub mod reflection;
pub mod templating;
pub mod parsing;
pub mod expressions;

use manifest::PaxManifest;

use std::fs;
use std::str::FromStr;
use std::collections::HashSet;

use include_dir::{Dir, DirEntry, include_dir};
use toml_edit::{Document, Item, value};
use std::path::{Path, PathBuf};
use std::process::Command;
use crate::manifest::{ComponentDefinition, ExpressionSpec};

//relative to pax_dir
pub const REEXPORTS_PARTIAL_RS_PATH: &str = "reexports.partial.rs";
/// Returns a sorted and de-duped list of combined_reexports.
fn generate_reexports_partial_rs(pax_dir: &PathBuf, manifest: &PaxManifest) {
    //traverse ComponentDefinitions in manifest
    //gather module_path and PascalIdentifier --
    //  handle `parser` module_path and any sub-paths
    //re-expose module_path::PascalIdentifier underneath `pax_reexports`
    //ensure that this partial.rs file is loaded included under the `pax_app` macro
    let mut reexport_components: Vec<String> = manifest.components.iter().map(|cd|{
        //e.g.: "some::module::path::SomePascalIdentifier"
        cd.1.module_path.clone() + "::" + &cd.1.pascal_identifier
    }).collect();

    let mut reexport_types : Vec<String> = manifest.components.iter().map(|cd|{
        cd.1.property_definitions.iter().map(|pm|{
            pm.fully_qualified_constituent_types.clone()
        }).flatten().collect::<Vec<_>>()
    }).flatten().collect::<Vec<_>>();

    let mut combined_reexports = reexport_components;
    combined_reexports.append(&mut reexport_types);
    combined_reexports.sort();

    let mut file_contents = "pub mod pax_reexports { \n".to_string();

    //Make combined_reexports unique by pouring into a Set and back
    let set: HashSet<_> = combined_reexports.drain(..).collect();
    combined_reexports.extend(set.into_iter());
    combined_reexports.sort();

    file_contents += &bundle_reexports_into_namespace_string(&combined_reexports);

    file_contents += "}";

    let path = pax_dir.join(Path::new(REEXPORTS_PARTIAL_RS_PATH));
    fs::write(path, file_contents);
}

fn bundle_reexports_into_namespace_string(sorted_reexports: &Vec<String>) -> String {

    //0. sort (expected to be passed sorted)
    //1. keep transient stack of nested namespaces.  For each export string (like pax::api::Size)
    //   - Split by "::"
    //   - if no `::`, or `crate::`, export at root of `pax_reexports`, i.e. empty stack
    //   - if `::`,
    //      - push onto stack the first n-1 identifiers as namespace
    //        - when pushing onto stack, write a `pub mod _identifier_ {`
    //      - when last element is reached, write a `pub use _identifier_;`
    //      - keep track of previous or next element, pop from stack for each of `n` mismatched prefix tokens
    //        - when popping from stack, write a `}`
    //        - empty stack entirely at end of vec

    //Author's note: if this logic must be refactored significantly, consider building a tree data structure & serializing it, instead
    //      of doubling down on this sorted/iterator/stack approach.  This ended up fairly arcane and brittle. -zb

    let mut namespace_stack = vec![];
    let mut output_string = "".to_string();

    //identify namespaceless or prelude-qualified types, e.g. `f64`
    fn is_reexport_namespaceless(symbols: &Vec<String>) -> bool {
        symbols.len() == 0
    }

    //identify `crate::*` reexports, e.g. `crate::HelloWorld`.  Note that the naive
    //implementation here will not support namespaces, thus requiring globally unique
    //symbol names for symbols exported from Pax project.
    fn is_reexport_crate_prefixed(symbols: &Vec<String>) -> bool {
        symbols[0].eq("crate")
    }

    fn get_tabs (i: usize) -> String {
        "\t".repeat(i + 1).to_string()
    };

    fn pop_and_write_brace(namespace_stack: &mut Vec<String>, output_string: &mut String){
        namespace_stack.pop();
        output_string.push_str(&*(get_tabs(namespace_stack.len()) + "}\n"));
    };

    fn dump_stack(namespace_stack: &mut Vec<String>, output_string: &mut String)  {
        while namespace_stack.len() > 0 {
            pop_and_write_brace(namespace_stack, output_string);
        }
    };

    sorted_reexports.iter().enumerate().for_each(|(i,pub_use)| {

        let symbols: Vec<String> = pub_use.split("::").map(|s|{s.to_string()}).collect();

        if is_reexport_namespaceless(&symbols) || is_reexport_crate_prefixed(&symbols) {
            //we can assume we're already at the root of the stack, thanks to the look-ahead stack-popping logic.
            assert!(namespace_stack.len() == 0);
            output_string += &*(get_tabs(namespace_stack.len()) + "pub use " + pub_use + ";\n");
        } else {
            //push necessary symbols to stack
            let starting_index = namespace_stack.len();
            for k in 0..((symbols.len() - 1) - namespace_stack.len()) {
                //k represents the offset `k` from `starting_index`, where `k + starting_index`
                //should be retrieved from `symbols` and pushed to `namespace_stack`
                let namespace_symbol = symbols.get(k + starting_index).unwrap().clone();
                output_string += &*(get_tabs(namespace_stack.len()) + "pub mod " + &namespace_symbol + " {\n");
                namespace_stack.push(namespace_symbol);
            }

            output_string += &*(get_tabs(namespace_stack.len()) + "pub use " + pub_use + ";\n");

            //look-ahead and pop stack as necessary
            match sorted_reexports.get(i + 1) {
                Some(next_reexport) => {
                    let next_symbols : Vec<String> = next_reexport.split("::").map(|s|{s.to_string()}).collect();
                    if is_reexport_crate_prefixed(&next_symbols) || is_reexport_namespaceless(&next_symbols) {
                        dump_stack(&mut namespace_stack, &mut output_string);
                    } else {
                        //for the CURRENT first n-1 symbols, check against same position in
                        //new_symbols.
                        //for the first mismatched symbol at i, pop k times, where k = (n-1)-i

                        let mut how_many_pops = None;
                        let n_minus_one = symbols.len() - 1;
                        symbols.iter().take(symbols.len() - 1).enumerate().for_each(|(i,symbol)|{
                            if let None = how_many_pops {
                                if let Some(next_symbol) = next_symbols.get(i) {
                                    if !next_symbol.eq(symbol) {
                                        how_many_pops = Some(n_minus_one - i);
                                    }
                                } else {
                                    how_many_pops = Some(n_minus_one - i);
                                }
                            }
                        });

                        if let Some(pops) = how_many_pops {
                            for i in 0..pops {
                                pop_and_write_brace(&mut namespace_stack, &mut output_string);
                            }
                        }
                    }
                },
                None => {
                    //we're at the end of the vec ??? dump stack and write braces
                    dump_stack(&mut namespace_stack, &mut output_string);
                }
            }
        }
    });

    output_string
}

fn update_property_prefixes_in_place(manifest: &mut PaxManifest, host_crate_info: &HostCrateInfo) {
    //update property types in-place
    manifest.components.iter_mut().for_each(|cd| {
        cd.1.property_definitions.iter_mut().for_each(|pm| {
            pm.property_type_info.pascalized_fully_qualified_type = pm.property_type_info.pascalized_fully_qualified_type.replace("{PREFIX}", "__");
            pm.property_type_info.fully_qualified_type = pm.property_type_info.fully_qualified_type.replace("{PREFIX}", &host_crate_info.import_prefix);
        });
    });
}


fn generate_properties_coproduct(pax_dir: &PathBuf, build_id: &str, manifest: &PaxManifest, host_crate_info: &HostCrateInfo) {

    let target_dir = pax_dir.join("properties-coproduct");
    clone_properties_coproduct_to_dot_pax(&target_dir).unwrap();

    let target_cargo_full_path = fs::canonicalize(target_dir.join("Cargo.toml")).unwrap();
    let mut target_cargo_toml_contents = toml_edit::Document::from_str(&fs::read_to_string(&target_cargo_full_path).unwrap()).unwrap();

    clean_dependencies_table_of_relative_paths(target_cargo_toml_contents["dependencies"].as_table_mut().unwrap());

    //insert new entry pointing to userland crate, where `pax_app` is defined
    std::mem::swap(
        target_cargo_toml_contents["dependencies"].get_mut(&host_crate_info.name).unwrap(),
        &mut Item::from_str("{ path=\"../..\" }").unwrap()
    );

    //write patched Cargo.toml
    fs::write(&target_cargo_full_path, &target_cargo_toml_contents.to_string());


    //build tuples for PropertiesCoproduct
    let mut properties_coproduct_tuples : Vec<(String, String)> = manifest.components.iter().map(|comp_def| {
        (
            comp_def.1.pascal_identifier.clone(),
            format!("{}{}{}{}", &host_crate_info.import_prefix, &comp_def.1.module_path.replace("crate", ""), {if comp_def.1.module_path == "crate" {""} else {"::"}}, &comp_def.1.pascal_identifier)
        )
    }).collect();
    let mut set: HashSet<(String, String)> = properties_coproduct_tuples.drain(..).collect();
    properties_coproduct_tuples.extend(set.into_iter());
    properties_coproduct_tuples.sort();



    //build tuples for TypesCoproduct
    // - include all Property types, representing all possible return types for Expressions
    // - include all T such that T is the iterator type for some Property<Vec<T>>
    let mut types_coproduct_tuples : Vec<(String, String)> = manifest.components.iter().map(|cd|{
        cd.1.property_definitions.iter().map(|pm|{
            (pm.property_type_info.pascalized_fully_qualified_type.clone(),
             pm.property_type_info.fully_qualified_type.clone())
        }).collect::<Vec<_>>()
    }).flatten().collect::<Vec<_>>();

    let mut set: HashSet<_> = types_coproduct_tuples.drain(..).collect();

    let BUILT_INS = vec![
        ("f64", "f64"),
        ("bool", "bool"),
        ("isize", "isize"),
        ("usize", "usize"),
        ("String", "String"),
        ("Vec_Rc_PropertiesCoproduct___", "std::vec::Vec<std::rc::Rc<PropertiesCoproduct>>"),
        ("Transform2D", "pax_runtime_api::Transform2D"),
        ("Size2D", "pax_runtime_api::Size2D"),
    ];

    BUILT_INS.iter().for_each(|builtin| {set.insert((builtin.0.to_string(), builtin.1.to_string()));});
    types_coproduct_tuples.extend(set.into_iter());
    types_coproduct_tuples.sort();

    //press template into String
    let generated_lib_rs = templating::press_template_codegen_properties_coproduct_lib(templating::TemplateArgsCodegenPropertiesCoproductLib {
        properties_coproduct_tuples,
        types_coproduct_tuples,
    });

    //write String to file
    fs::write(target_dir.join("src/lib.rs"), generated_lib_rs);

}

fn generate_cartridge_definition(pax_dir: &PathBuf, build_id: &str, manifest: &PaxManifest, host_crate_info: &HostCrateInfo) {
    let target_dir = pax_dir.join("cartridge");
    clone_cartridge_to_dot_pax(&target_dir);

    let target_cargo_full_path = fs::canonicalize(target_dir.join("Cargo.toml")).unwrap();
    let mut target_cargo_toml_contents = toml_edit::Document::from_str(&fs::read_to_string(&target_cargo_full_path).unwrap()).unwrap();

    clean_dependencies_table_of_relative_paths(target_cargo_toml_contents["dependencies"].as_table_mut().unwrap());

    //insert new entry pointing to userland crate, where `pax_app` is defined
    std::mem::swap(
        target_cargo_toml_contents["dependencies"].get_mut(&host_crate_info.name).unwrap(),
        &mut Item::from_str("{ path=\"../..\" }").unwrap()
    );

    //write patched Cargo.toml
    fs::write(&target_cargo_full_path, &target_cargo_toml_contents.to_string());

    //Gather all fully_qualified_constituent_types from manifest; prepend with re-export prefix; make unique
    let IMPORT_PREFIX = format!("{}::pax_reexports::", host_crate_info.identifier);
    let mut imports : Vec<String> = manifest.components.values().map(|comp_def: &ComponentDefinition|{
        comp_def.property_definitions.iter().map(|prop_def|{
            prop_def.fully_qualified_constituent_types.iter().map(|fqct|{
                IMPORT_PREFIX.clone() + fqct
            }).collect::<Vec<String>>()
        }).flatten().collect::<Vec<String>>()
    }).flatten().collect::<Vec<String>>();
    let unique_imports: HashSet<String> = imports.drain(..).collect();
    imports.extend(unique_imports.into_iter());

    let primitive_imports = vec![];//TODO!

    let consts = vec![];//TODO!

    //Traverse component tree starting at root
    //build a N/PIT in memory for each component (maybe this can be automatically serialized for component factories?)
    // handle each kind of attribute:
    //   Literal(String),
    //      inline into N/PIT
    //   Expression(String),
    //      pencil in the ID; handle the expression separately (build ExpressionSpec & friends)
    //   Identifier(String),
    //      syntactic sugar for an expression with a single dependency, returning that dependency
    //   EventBindingTarget(String),
    //      ensure this gets added to the HandlerRegistry for this component; rely on ugly error messages for now
    //
    // for serialization to RIL, generate InstantiationArgs for each node, special-casing built-ins like Repeat, Slot
    //
    // Also decide whether to join settings blocks in this work
    //
    // Compile expressions during traversal, keeping track of "compile-time stack" for symbol resolution
    //   If `const` is bit off for this work, must first populate symbols via pax_const => PaxManifest
    //     -- must also choose scoping rules; probably just component-level scoping for now
    //
    // Throw errors when symbols in expressions cannot be resolved; ensure path forward to developer-friendly error messages
    //     For reference, Rust's message is:
    //  error[E0425]: cannot find value `not_defined` in this scope
    //         --> pax-compiler/src/main.rs:404:13
    //          |
    //      404 |     let y = not_defined + 6;
    //          |             ^^^^^^^^^^^ not found in this scope
    //     Python uses:
    // NameError: name 'z' is not defined
    //     JavaScript uses:
    // Uncaught ReferenceError: not_defined is not defined

    let expression_specs = manifest.expression_specs.as_ref().unwrap().values().map(|es: &ExpressionSpec|{es.clone()}).collect();

    let component_factories_literal = vec![];//TODO!

    //press template into String
    let generated_lib_rs = templating::press_template_codegen_cartridge_lib(templating::TemplateArgsCodegenCartridgeLib {
        imports,
        primitive_imports,
        consts,
        expression_specs,
        component_factories_literal,
    });

    //write String to file
    fs::write(target_dir.join("src/lib.rs"), generated_lib_rs);

}

fn clean_dependencies_table_of_relative_paths(dependencies: &mut toml_edit::Table) {
    dependencies.iter_mut().for_each(|dep| {
        match dep.1.get_mut("path") {
            Some(existing_path) => {
                std::mem::swap(
                    existing_path,
                    &mut Item::None,
                );
            },
            _ => {}
        }
    });
}

fn generate_chassis_cargo_toml(pax_dir: &PathBuf, target: &RunTarget, build_id: &str, manifest: &PaxManifest) {
    //1. clone (git or raw fs) pax-chassis-whatever into .pax/chassis/
    let chassis_dir = pax_dir.join("chassis");
    std::fs::create_dir_all(&chassis_dir).expect("Failed to create chassis directory.  Check filesystem permissions?");

    let target_str : &str = target.into();
    let relative_chassis_specific_target_dir = chassis_dir.join(target_str);

    clone_target_chassis_to_dot_pax(&relative_chassis_specific_target_dir, target_str);

    //2. patch Cargo.toml
    let existing_cargo_toml_path = fs::canonicalize(relative_chassis_specific_target_dir.join("Cargo.toml")).unwrap();
    let mut existing_cargo_toml = toml_edit::Document::from_str(&fs::read_to_string(&existing_cargo_toml_path).unwrap()).unwrap();

    //remove all relative `path` entries from dependencies, so that we may patch.
    clean_dependencies_table_of_relative_paths(existing_cargo_toml["dependencies"].as_table_mut().unwrap());

    //add `patch`
    let mut patch_table = toml_edit::table();
    patch_table["pax-cartridge"]["path"] = toml_edit::value("../../cartridge");
    patch_table["pax-properties-coproduct"]["path"] = toml_edit::value("../../properties-coproduct");
    existing_cargo_toml.insert("patch.crates-io", patch_table);

    //3. write Cargo.toml back to disk & done
    //   hack out the double-quotes inserted by toml_edit along the way
    fs::write(existing_cargo_toml_path, existing_cargo_toml.to_string().replace("\"patch.crates-io\"", "patch.crates-io") );
}

static CHASSIS_MACOS_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/../pax-chassis-macos");
//NOTE: including this whole pax-chassis-web directory, plus node_modules, adds >100MB to the size of the
//      compiler binary; also extends build times for Web and build times for pax-compiler itself.
//      These are all development dependencies, namely around webpack/typescript -- this could be
//      improved with a "production build" of `pax-chassis-web` that gets included into the compiler
static CHASSIS_WEB_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/../pax-chassis-web");
/// Clone a copy of the relevant chassis (and dev harness) to the local .pax directory
/// The chassis is the final compiled Rust library (thus the point where `patch`es must occur)
/// and the encapsulated dev harness is the actual dev executable
fn clone_target_chassis_to_dot_pax(relative_chassis_specific_target_dir: &PathBuf, target_str: &str) -> std::io::Result<()> {

    // fs::remove_dir_all(&relative_chassis_specific_target_dir);
    fs::create_dir_all(&relative_chassis_specific_target_dir);

    //Note: zb spent too long tangling with this -- seems like fs::remove* and fs::create* work
    //      only with the relative path, while Dir::extract requires a canonicalized path.  At least: this works on macOS,
    //      and failed silently/partially in all explored configurations until this one
    let chassis_specific_dir = fs::canonicalize(&relative_chassis_specific_target_dir).expect("Invalid path");

    println!("Cloning {} chassis to {:?}", target_str, chassis_specific_dir);
    match RunTarget::from(target_str) {
        RunTarget::MacOS => {
            CHASSIS_MACOS_DIR.extract(&chassis_specific_dir);
            //HACK: patch the relative directory for the cdylib, because in a rust monorepo the `target` dir
            //      is at the monorepo root, while in this isolated project it will be in `pax-chassis-macos`.
            let pbx_path = &chassis_specific_dir.join("pax-dev-harness-macos").join("pax-dev-harness-macos.xcodeproj").join("project.pbxproj");
            fs::write(pbx_path, fs::read_to_string(pbx_path).unwrap().replace("../../target", "../target"))
        }
        RunTarget::Web => {
            CHASSIS_WEB_DIR.extract(&chassis_specific_dir)
        }
    }
}

static CARTRIDGE_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/../pax-cartridge");
/// Clone a copy of the relevant chassis (and dev harness) to the local .pax directory
/// The chassis is the final compiled Rust library (thus the point where `patch`es must occur)
/// and the encapsulated dev harness is the actual dev executable
fn clone_cartridge_to_dot_pax(relative_cartridge_target_dir: &PathBuf) -> std::io::Result<()> {
    // fs::remove_dir_all(&relative_cartridge_target_dir);
    fs::create_dir_all(&relative_cartridge_target_dir);

    let target_dir = fs::canonicalize(&relative_cartridge_target_dir).expect("Invalid path for generated pax cartridge");

    println!("Cloning cartridge to {:?}", target_dir);

    CARTRIDGE_DIR.extract(&target_dir)
}


static PROPERTIES_COPRODUCT_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/../pax-properties-coproduct");
/// Clone a copy of the relevant chassis (and dev harness) to the local .pax directory
/// The chassis is the final compiled Rust library (thus the point where `patch`es must occur)
/// and the encapsulated dev harness is the actual dev executable
fn clone_properties_coproduct_to_dot_pax(relative_cartridge_target_dir: &PathBuf) -> std::io::Result<()> {
    // fs::remove_dir_all(&relative_cartridge_target_dir);
    fs::create_dir_all(&relative_cartridge_target_dir);

    let target_dir = fs::canonicalize(&relative_cartridge_target_dir).expect("Invalid path for generated pax cartridge");

    println!("Cloning properties coproduct to {:?}", target_dir);

    PROPERTIES_COPRODUCT_DIR.extract(&target_dir)
}

fn get_or_create_pax_directory(working_dir: &str) -> PathBuf {
    let mut working_path = std::path::Path::new(working_dir).join(".pax");
    std::fs::create_dir_all( &working_path);
    working_path
}
const TMP_DIRECTORY_NAME: &str = "tmp";
fn get_or_create_pax_tmp_directory(working_dir: &str) -> PathBuf {
    let tmp = Path::new(&get_or_create_pax_directory(working_dir)).join(TMP_DIRECTORY_NAME);
    std::fs::create_dir_all( &tmp);
    tmp
}

/// Pulled from host Cargo.toml
struct HostCrateInfo {
    /// for example: `pax-example`
    name: String,
    /// for example: `pax_example`
    identifier: String,
    /// for example: `some_crate::pax_reexports`,
    import_prefix: String,
}

fn get_host_crate_info(cargo_toml_path: &Path) -> HostCrateInfo {
    let existing_cargo_toml = toml_edit::Document::from_str(&fs::read_to_string(
        fs::canonicalize(cargo_toml_path).unwrap()).unwrap()).expect("Error loading host Cargo.toml");

    let name = existing_cargo_toml["package"]["name"].as_str().unwrap().to_string();
    let identifier = name.replace("-", "_"); //NOTE: perhaps this could be less naive?

    let import_prefix = format!("{}::pax_reexports::", &identifier);

    HostCrateInfo {
        name,
        identifier,
        import_prefix,
    }
}

static TEMPLATE_DIR: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/templates");

/// For the specified file path or current working directory, first compile Pax project,
/// then run it with a patched build of the `chassis` appropriate for the specified platform
pub fn perform_build(ctx: RunContext, should_also_run: bool) -> Result<(), ()> {

    println!("Performing run");

    let pax_dir = get_or_create_pax_directory(&ctx.path);
    let tmp_dir =  get_or_create_pax_tmp_directory(&ctx.path);

    //FUTURE: handle stand-alone .pax files

    // Run parser bin from host project with `--features parser`
    let cargo_run_parser_process = Command::new("cargo")
        .current_dir(&ctx.path)
        .arg("run")
        .arg("--features")
        .arg("parser")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("failed to execute parser binary");

    let output = cargo_run_parser_process
        .wait_with_output().unwrap();

    let out = String::from_utf8(output.stdout).unwrap();
    let _err = String::from_utf8(output.stderr).unwrap();

    //FUTURE: handle parsing errors here
    assert_eq!(output.status.code().unwrap(), 0);

    let mut manifest : PaxManifest = serde_json::from_str(&out).expect(&format!("Malformed JSON from parser: {}", &out));

    let host_cargo_toml_path = Path::new(&ctx.path).join("Cargo.toml");
    let host_crate_info = get_host_crate_info(&host_cargo_toml_path);
    update_property_prefixes_in_place(&mut manifest, &host_crate_info);

    expressions::compile_all_expressions(&mut manifest);

    let build_id = uuid::Uuid::new_v4().to_string();

    generate_reexports_partial_rs(&pax_dir, &manifest);
    generate_properties_coproduct(&pax_dir, &build_id, &manifest, &host_crate_info);
    generate_cartridge_definition(&pax_dir, &build_id, &manifest, &host_crate_info);
    generate_chassis_cargo_toml(&pax_dir, &ctx.target, &build_id, &manifest);

    //7. Build the appropriate `chassis` from source, with the patched `Cargo.toml`, Properties Coproduct, and Cartridge from above
    build_chassis_with_cartridge(&pax_dir);

    //8a::run: Run dev harness, with freshly built chassis plugged in
    //8b::compile: Build production harness, with freshly built chassis plugged in

    //see pax-compiler-sequence-diagram.png

    Ok(())
}


fn build_chassis_with_cartridge(pax_dir: &PathBuf) {
    todo!();



    //string together a shell call like the following:
    // let cargo_run_parser_process = Command::new("cargo")
    //     .current_dir(&ctx.path)
    //     .arg("run")
    //     .arg("--features")
    //     .arg("parser")
    //     .stdout(std::process::Stdio::piped())
    //     .stderr(std::process::Stdio::piped())
    //     .spawn()
    //     .expect("failed to execute parser binary");
}


pub struct RunContext {
    pub target: RunTarget,
    pub path: String,
}

pub enum RunTarget {
    MacOS,
    Web,
}

impl From<&str> for RunTarget {
    fn from(input: &str) -> Self {
        match input.to_lowercase().as_str() {
            "macos" => {
                RunTarget::MacOS
            },
            "web" => {
                RunTarget::Web
            }
            _ => {unreachable!()}
        }
    }
}

impl<'a> Into<&'a str> for &'a RunTarget {
    fn into(self) -> &'a str {
        match self {
            RunTarget::Web => {
                "Web"
            },
            RunTarget::MacOS => {
                "MacOS"
            },
            _ => {
                unreachable!();
            }
        }
    }
}