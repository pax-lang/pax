#[macro_use]
extern crate lazy_static;


use pax::*;

pub struct DeeperStruct {
    a: i64,
    b: &'static str,
}

//Note re: dependencies —
//  - The central PropertiesCoproduct _depends on_ this definition, in order to wrap it into the PropertiesCoproduct
//  - This means that this file cannot directly rely on pax-properties-coproduct.  To do so would introduce a cyclic dep.
//    In particular, be mindful of this when designing macro expansion

//could make file ref. explicit: #[pax(file="lib.pax")]
//in absence, .pax file path is inferred by source name (and `is_present(inline_pax)`)
//e.g. lib.rs -> try to load lib.pax.  don't try to load .pax if inline_pax is present

//#[pax] was here

#[cfg(feature="derive-manifest")]
lazy_static! {
    //TODO: retrieve this from TokenStream and retire lazy_static!
    static ref this : String = String::from("Root");
}

pub struct Root {
    //rewrite to pub `num_clicks : Property<i64>` etc. AND register metadata with dev server
    pub num_clicks : i64,
    pub current_rotation: f64,
    pub deeper_struct: DeeperStruct,
}


#[cfg(feature="derive-manifest")]
use pax::message::ComponentDefinition;
#[cfg(feature="derive-manifest")]
use pax::compiletime;
#[cfg(feature="derive-manifest")]
use std::collections::HashSet;
#[cfg(feature="derive-manifest")]
use pax::compiletime::ManifestContext;




#[cfg(feature="derive-manifest")]
lazy_static! {
    static ref source_id : String = compiletime::get_uuid();
}
#[cfg(feature="derive-manifest")]
lazy_static! {
    static ref this : String = String::from("Root");
}

#[cfg(feature="derive-manifest")]
pub fn main() {
    let mut ctx = ManifestContext{
        visited_source_ids: HashSet::new(),
        component_definitions: vec![],
    };
    ctx = Root::get_manifest(ctx);


}
#[cfg(feature="derive-manifest")]
impl Root {
    pub fn parse_to_manifest(mut ctx: ManifestContext) -> ManifestContext {

        match ctx.visited_source_ids.get(&file_id) {
            None => {
                //First time visiting this file/source — parse the relevant contents
                //then recurse through child nodes, unrolled here in the macro as
                //parsed from the template
                ctx.visited_source_ids.insert(file_id.clone());

                ctx.component_definitions.push(
                    compiletime::process_file_for_component_definition(&this,file!(), module_path!());
                );

                //******** dynamic macro logic here
                ctx = Spread::get_manifest(ctx);
                ctx = Rectangle::get_manifest(ctx);
                ctx = Group::get_manifest(ctx);
                ctx = Text::get_manifest(ctx);
                //******** end dynamic macro logic

                ctx
            },
            _ => {ctx} //early return; this file has already been parsed
        }


        /*
        <Spread id="main-spread">
            <Rectangle id="rect-1" />
            <Rectangle id="rect-2" />
            <Group>
                <Text id="label" content="Hello!" />
                <Rectangle id="rect-3" />
            </Group>
        </Spread>
         */
        //code-gen manifest recursion


        //note: duplicates are managed by
        //      the file_id hack — can keep a registry in
        //      o



        // file!
        // module!
        // find children; recurse get_manifest()
        // in future: get schema of methods, types of properties
    }
}


impl Root {

    pub fn new() -> Self {
        Self {
            //Default values.  Could shorthand this into a macro via PAXEL
            num_clicks: 0,
            current_rotation: 0.0,
            deeper_struct: DeeperStruct {
                a: 100,
                b: "Profundo!",
            }
        }
    }

    //On click, increment num_clicks and update the rotation

    //Note the userland ergonomics here, using .get() and .set()
    //vs. the constructor and struct definition of bare types (e.g. i64, which doesn't have a .get() or .set() method)
    //Approaches:
    // - rewrite the struct at macro time; also rewrite the constructor
    // - inject something other than self into increment_clicker, including a .gettable and .settable wrapper
    //   around (note that this injected struct, if it's going to have the pattern struct.num_clicks.set, will
    //   still require some codegen; can't be achieved with generics alone


    // pub fn increment_clicker(&mut self, args: ClickArgs) {
    //     self.num_clicks.set(self.num_clicks + 1);
    //     self.current_rotation.setTween( //also: setTweenLater, to enqueue a tween after the current (if any) is done
    //         self.num_clicks.get() * 3.14159 / 4,
    //         Tween {duration: 1000, curve: Tween::Ease}
    //     );
    // }

}


/* Approaches for dirty-handling of properties:
    - Check dataframes on each tick (brute-force)
    - inject a setter, ideally with primitive ergonomics (`self.x = self.x + 1`)
        probably done with a macro decorating the struct field
        - setter(a): generate a `set_field_name<T>(new: T)` method for each decorated `field_name: T`
       ***setter(b):   `num_clicks: T` becomes `self.num_clicks.get() //-> T` and `self.num_clicks.set(new: T)`
                       in the expression language, `num_clicks` automatically unwraps `get()`
                       `.get()` feels fine for Rust ergonomics, in line with `unwrap()`
                       `.set(new: T)` is also not the worst, even if it could be better.
                       In TS we can have better ergonomics with `properties`
 */




//DONE: is all descendent property access via Actions + selectors? `$('#some-desc').some_property`
//      or do we need a way to support declaring desc. properties?
//      We do NOT need a way to declar desc. properties here — because they are declared in the
//      `properties` blocks of .dash