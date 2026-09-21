//! JavaScript snippets injected into the site's project editor. These call the
//! editor's own (undocumented) functions — the same ones the reference Python
//! tool drives — so the exact names here are load-bearing.
//!
//! Snippets with no runtime arguments are `&'static str` so the polling loops in
//! `driver` don't allocate a fresh `String` on every tick.

/// The editor hosts the size/mode controls inside this same-origin iframe. We
/// reach into it from the main frame rather than switching contexts.
const EDITOR_FRAME: &str = "document.getElementById('sysifm_loginFrame').contentWindow";

/// Checks computed style rather than `offsetParent`, which is `null` for
/// `position:fixed` overlays even when they are visible. Keep all loading-marker
/// probes on this expression: MPC leaves hidden marker elements in the DOM.
macro_rules! element_visible {
    () => {
        "function(e){if(!e)return false;var s=getComputedStyle(e);return s.display!=='none'&&s.visibility!=='hidden';}"
    };
}

const ELEMENT_VISIBLE: &str = element_visible!();

/// Is the page's loading spinner currently displayed?
pub const LOADING_SPINNER_VISIBLE: &str = concat!(
    "(function(){var visible=",
    element_visible!(),
    ";return visible(document.getElementById('sysdiv_wait'));})()"
);

/// Whether the product configurator has finished rebuilding its requested
/// controls. The loading marker is inserted while a stock, quantity bracket, or
/// foil change causes the site to replace the selects asynchronously.
pub fn product_controls_ready(
    cardstock_dropdown_id: &str,
    quantity_dropdown_id: Option<&str>,
    print_type_dropdown_id: Option<&str>,
) -> String {
    let required_controls = [
        Some(cardstock_dropdown_id),
        quantity_dropdown_id,
        print_type_dropdown_id,
    ]
    .into_iter()
    .flatten()
    .map(|id| format!("'{id}'"))
    .collect::<Vec<_>>()
    .join(",");
    format!(
        "(function(){{var visible={ELEMENT_VISIBLE};return document.readyState!=='loading'&&\
         !visible(document.querySelector('.productPageLoading'))&&\
         [{required_controls}].every(function(id){{\
         var e=document.getElementById(id);return e&&!e.disabled;}});}})()"
    )
}

pub const NEXT_STEP: &str = "oDesign.setNextStep();";

/// Whether the personalized editor's iframe and its quantity control are ready
/// for use. `doPersonalize` navigates, so the outer page being idle alone is
/// not enough to make this nested document available.
pub const EDITOR_QUANTITY_READY: &str =
    "(function(){try{var f=document.getElementById('sysifm_loginFrame');\
     return document.readyState!=='loading'&&!!f&&!!f.contentWindow&&\
     !!f.contentWindow.document.getElementById('txt_card_number');\
     }catch(e){return false;}})()";

/// The MPC backs page and its nested image-mode chooser. Pathname-only checks
/// deliberately avoid carrying URL query data (which can contain account state)
/// into diagnostics.
/// Return a safe, finite description of whether the MPC back-mode chooser can
/// be used or opened. This is read-only: callers poll it before making the one
/// mutation that opens the chooser.
pub const BACK_CHOOSER_STATE: &str = concat!(
    "(function(){var visible=", element_visible!(), ";var back='/products/playingcard/design/dn_playingcards_back_dynamic.aspx',chooser='/products/playingcard/design/dn_playingcards_mode_nb.aspx';",
    "if(location.pathname!==back)return 'wrong-stage';var f=document.getElementById('sysifm_loginFrame');",
    "var box=document.getElementById('sysdiv_loginFrame');if(f&&visible(f)&&box&&visible(box)){",
    "try{if(!f.contentWindow||f.contentWindow.location.pathname!==chooser)return 'stale-chooser';",
    "if(typeof f.contentWindow.setMode==='function'&&f.contentWindow.oRenderFeature&&typeof f.contentWindow.oRenderFeature==='object')return 'ready';return 'loading';}catch(e){return 'stale-chooser';}}",
    "var p=window.PageLayout&&PageLayout.prototype,c=p&&p.parameters&&p.parameters.ChangeDesignCount,",
    "url=c&&(c.url||c.Url||c.URL),width=c&&Number(c.width||c.Width),height=c&&Number(c.height||c.Height);",
    "if(!p||typeof p.renderDesignCount!=='function'||typeof window.open!=='function'||typeof url!=='string'||!/^\\//.test(url)||!isFinite(width)||width<=0||!isFinite(height)||height<=0)return 'uninitialized-layout';return 'openable';})()"
);

/// Open the image-mode chooser once the read-only state probe found an
/// initialized backs layout. `renderDesignCount` opens the chooser; it does not
/// materialise the image slots. Do not catch its exceptions: opening is a
/// mutation and an uncertain result must not be replayed.
pub fn open_back_chooser() -> String {
    format!(
        "(function(){{if({BACK_CHOOSER_STATE}!==\"openable\")throw new Error('back chooser is no longer openable');PageLayout.prototype.renderDesignCount();}})()"
    )
}

fn set_back_mode(mode: u8) -> String {
    format!(
        "(function(){{if({BACK_CHOOSER_STATE}!==\"ready\")throw new Error('back chooser is not ready');{EDITOR_FRAME}.setMode('ImageText', {mode});}})()"
    )
}

/// Configure the current (front) editor to use a distinct image per card. The
/// fronts flow owns readiness checks for its editor frame; it must not inherit
/// the backs-page chooser guard.
pub const DIFFERENT_IMAGES: &str =
    "(function(){document.getElementById('sysifm_loginFrame').contentWindow.setMode('ImageText', 0);})()";

/// Configure the current (front) editor to use a distinct image per card.
pub fn different_images() -> String {
    DIFFERENT_IMAGES.to_owned()
}

/// Configure the current back face to use a distinct image per card.
pub fn different_back_images() -> String {
    set_back_mode(0)
}

/// Configure the current back face to reuse one image for every card.
pub fn same_back_images() -> String {
    set_back_mode(1)
}

pub const UPLOAD_STATUS_IS_UPLOADING: &str =
    "(typeof oDesignImage !== 'undefined' && oDesignImage.UploadStatus === 'Uploading')";

/// The semicolon-separated list of PIDs already uploaded to the project.
pub const UPLOADED_PIDS: &str =
    "(typeof oDesignImage !== 'undefined' ? (oDesignImage.dn_getImageList() || '') : '')";

/// The number of images uploaded to the project so far.
pub const UPLOADED_IMAGE_COUNT: &str =
    "(typeof oDesignImage !== 'undefined' ? (oDesignImage.dn_getImageList() || '').split(';').filter(Boolean).length : 0)";

/// Is a (possibly dotted) JavaScript symbol defined in the given window? Wrapped
/// in try/catch so a dotted path like `oDesign.setNextStep` returns false rather
/// than throwing when an intermediate object is missing.
pub fn is_defined(symbol: &str, in_frame: bool) -> String {
    let scope = if in_frame { EDITOR_FRAME } else { "window" };
    format!("(function(){{try{{return typeof {scope}.{symbol} !== 'undefined';}}catch(e){{return false;}}}})()")
}

pub fn do_personalize(accept_settings_url: &str) -> String {
    format!("doPersonalize('{accept_settings_url}');")
}

pub fn set_card_quantity(quantity: u32) -> String {
    format!(
        "(function(){{var q={EDITOR_FRAME}.document.getElementById('txt_card_number');\
         q.value='{quantity}';q.dispatchEvent(new Event('input'));\
         q.dispatchEvent(new Event('change'));return true;}})()"
    )
}

/// Select the dropdown option whose visible text matches, comparing on
/// whitespace-normalised text (Selenium's `select_by_visible_text` semantics).
pub fn select_by_text(dropdown_id: &str, text: &str) -> String {
    let text = escape(text);
    format!(
        "(function(){{var s=document.getElementById('{dropdown_id}');\
         var want='{text}'.replace(/\\s+/g,' ').trim();\
         for(var i=0;i<s.options.length;i++){{\
         if(s.options[i].text.replace(/\\s+/g,' ').trim()===want){{\
         s.selectedIndex=i;s.dispatchEvent(new Event('change'));return true;}}}}\
         return false;}})()"
    )
}

pub fn select_by_value(dropdown_id: &str, value: &str) -> String {
    let value = escape(value);
    format!(
        "(function(){{var s=document.getElementById('{dropdown_id}');s.value='{value}';\
         s.dispatchEvent(new Event('change'));return s.value==='{value}';}})()"
    )
}

/// Verify the settled configurator still holds the requested stock and smallest
/// fitting quantity bracket. `foil_value` is optional: non-foil sources/sites
/// leave their normal print-type selection untouched.
pub fn product_configuration_matches(
    cardstock_dropdown_id: &str,
    stock: &str,
    quantity_dropdown_id: &str,
    quantity: u32,
    print_type_dropdown_id: &str,
    foil_value: Option<&str>,
) -> String {
    let stock = escape(stock);
    let foil_check = match foil_value {
        Some(value) => format!(
            "&&document.getElementById('{print_type_dropdown_id}').value==='{}'",
            escape(value)
        ),
        None => String::new(),
    };
    format!(
        "(function(){{var stock=document.getElementById('{cardstock_dropdown_id}');\
         var quantity=document.getElementById('{quantity_dropdown_id}');\
         var normalise=function(s){{return s.replace(/\\s+/g,' ').trim();}};\
         if(!stock||!quantity||normalise(stock.options[stock.selectedIndex].text)!==normalise('{stock}'))return false;\
         var selected=parseInt(quantity.value,10);if(isNaN(selected)||selected<{quantity})return false;\
         for(var i=0;i<quantity.options.length;i++){{var value=parseInt(quantity.options[i].value,10);\
         if(!isNaN(value)&&value>={quantity}&&value<selected)return false;}}\
         return true{foil_check};}})()"
    )
}

/// Choose the smallest bracket option whose value is at least `quantity`.
pub fn select_smallest_bracket(dropdown_id: &str, quantity: u32) -> String {
    format!(
        "(function(){{var s=document.getElementById('{dropdown_id}');var best=null,bi=-1;\
         for(var i=0;i<s.options.length;i++){{var v=parseInt(s.options[i].value,10);\
         if(!isNaN(v)&&v>={quantity}&&(best===null||v<best)){{best=v;bi=i;}}}}\
         if(bi<0)return false;s.selectedIndex=bi;s.dispatchEvent(new Event('change'));return true;}})()"
    )
}

/// Whether the initial backs layout has a second slot. The editor may not have
/// created `PageLayout` yet on this transition; that is its legacy one-image
/// layout, not a reason to block mode selection.
pub const INITIAL_LAYOUT_HAS_SLOT_ONE: &str =
    "(function(){try{return PageLayout.prototype.getElement3('dnImg','1') != null;}catch(e){return false;}})()";

/// Does the editor currently have a slot element for `slot`?
pub fn slot_exists(slot: u32) -> String {
    format!("PageLayout.prototype.getElement3('dnImg', '{slot}') != null")
}

/// The PID currently occupying `slot`, or an empty string.
pub fn pid_in_slot(slot: u32) -> String {
    format!(
        "(function(){{var e=PageLayout.prototype.getElement3('dnImg','{slot}');\
         return e ? (e.getAttribute('pid') || '') : '';}})()"
    )
}

/// Drop the image identified by `pid` into `slot`.
pub fn insert_into_slot(slot: u32, pid: &str) -> String {
    format!(
        "PageLayout.prototype.applyDragPhoto(\
         PageLayout.prototype.getElement3('dnImg','{slot}'), 0, '{pid}');"
    )
}

fn escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('\'', "\\'")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(script: &str, snippet: &str) -> String {
        let output = std::process::Command::new("node")
            .args(["-e", script, snippet])
            .output()
            .expect("node is required for JavaScript behavior tests");
        assert!(
            output.status.success(),
            "Node execution failed for {snippet:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout)
            .expect("node output must be UTF-8")
            .trim()
            .to_owned()
    }

    fn evaluate(snippet: &str, setup: &str) -> String {
        node(
            &format!("{setup}\nprocess.stdout.write(String(eval(process.argv[1])));"),
            snippet,
        )
    }

    fn execute_and_observe(snippet: &str, setup: &str, observation: &str) -> String {
        node(
            &format!("{setup}\neval(process.argv[1]);process.stdout.write(String({observation}));"),
            snippet,
        )
    }

    #[test]
    fn all_public_snippets_parse_as_cdp_scripts() {
        let snippets = vec![
            LOADING_SPINNER_VISIBLE.to_owned(),
            product_controls_ready("paper", None, None),
            product_controls_ready("paper", Some("quantity"), Some("effect")),
            NEXT_STEP.to_owned(),
            EDITOR_QUANTITY_READY.to_owned(),
            BACK_CHOOSER_STATE.to_owned(),
            open_back_chooser(),
            different_images(),
            different_back_images(),
            same_back_images(),
            UPLOAD_STATUS_IS_UPLOADING.to_owned(),
            UPLOADED_PIDS.to_owned(),
            UPLOADED_IMAGE_COUNT.to_owned(),
            is_defined("oDesign.setNextStep", false),
            is_defined("editor.setMode", true),
            do_personalize("/accept-settings"),
            set_card_quantity(54),
            select_by_text("paper", "Super  Gloss"),
            select_by_value("effect", "EF_055"),
            product_configuration_matches("paper", "S33", "quantity", 54, "effect", None),
            product_configuration_matches("paper", "S33", "quantity", 54, "effect", Some("EF_055")),
            select_smallest_bracket("quantity", 54),
            INITIAL_LAYOUT_HAS_SLOT_ONE.to_owned(),
            slot_exists(1),
            pid_in_slot(1),
            insert_into_slot(1, "pid-1"),
        ];
        for snippet in snippets {
            // CDP evaluates scripts, not function bodies: vm.Script catches syntax
            // errors such as a top-level `return` while preserving that grammar.
            node(
                "const vm=require('vm');new vm.Script(process.argv[1]);",
                &snippet,
            );
        }
    }

    #[test]
    fn spinner_visibility_handles_missing_and_hidden_markers() {
        for (marker, expected) in [
            ("null", "false"),
            ("{style:{display:'none',visibility:'visible'}}", "false"),
            ("{style:{display:'block',visibility:'hidden'}}", "false"),
            ("{style:{display:'block',visibility:'visible'}}", "true"),
        ] {
            assert_eq!(
                evaluate(
                    LOADING_SPINNER_VISIBLE,
                    &format!(
                        "const marker={marker};global.document={{getElementById:()=>marker}};global.getComputedStyle=e=>e.style;"
                    ),
                ),
                expected
            );
        }
    }

    #[test]
    fn readiness_covers_stock_bracket_and_foil_stages() {
        let setup = "const controls={paper:{disabled:false},quantity:{disabled:false},effect:{disabled:false}};const loader={style:{display:'none',visibility:'visible'}};global.document={readyState:'complete',querySelector:()=>loader,getElementById:id=>controls[id]||null};global.getComputedStyle=e=>e.style;";
        assert_eq!(
            evaluate(&product_controls_ready("paper", None, None), setup),
            "true"
        );
        assert_eq!(
            evaluate(
                &product_controls_ready("paper", Some("quantity"), None),
                setup
            ),
            "true"
        );
        assert_eq!(
            evaluate(
                &product_controls_ready("paper", Some("quantity"), Some("effect")),
                setup
            ),
            "true"
        );
        assert_eq!(
            evaluate(
                &product_controls_ready("paper", Some("quantity"), Some("effect")),
                "const controls={paper:{disabled:false},quantity:{disabled:false},effect:{disabled:false}};const loader={style:{display:'block',visibility:'visible'}};global.document={readyState:'complete',querySelector:()=>loader,getElementById:id=>controls[id]||null};global.getComputedStyle=e=>e.style;"
            ),
            "false"
        );
    }

    #[test]
    fn readiness_rejects_loading_or_missing_or_disabled_required_controls() {
        let ready = |controls: &str, state: &str, required_quantity, required_foil| {
            evaluate(
                &product_controls_ready("paper", required_quantity, required_foil),
                &format!(
                    "const controls={controls};const loader={{style:{{display:'none',visibility:'visible'}}}};global.document={{readyState:'{state}',querySelector:()=>loader,getElementById:id=>controls[id]||null}};global.getComputedStyle=e=>e.style;"
                ),
            )
        };

        assert_eq!(
            ready(
                "{quantity:{disabled:false},effect:{disabled:false}}",
                "complete",
                Some("quantity"),
                Some("effect")
            ),
            "false"
        );
        assert_eq!(
            ready("{paper:{disabled:true}}", "complete", None, None),
            "false"
        );
        assert_eq!(
            ready(
                "{paper:{disabled:false}}",
                "complete",
                Some("quantity"),
                None
            ),
            "false"
        );
        assert_eq!(
            ready(
                "{paper:{disabled:false},quantity:{disabled:true}}",
                "complete",
                Some("quantity"),
                None
            ),
            "false"
        );
        assert_eq!(
            ready(
                "{paper:{disabled:false},quantity:{disabled:false}}",
                "complete",
                Some("quantity"),
                Some("effect")
            ),
            "false"
        );
        assert_eq!(
            ready(
                "{paper:{disabled:false},quantity:{disabled:false},effect:{disabled:true}}",
                "complete",
                Some("quantity"),
                Some("effect")
            ),
            "false"
        );
        assert_eq!(
            ready("{paper:{disabled:false}}", "loading", None, None),
            "false"
        );
    }

    #[test]
    fn readiness_only_requires_controls_for_the_current_stage() {
        assert_eq!(
            evaluate(
                &product_controls_ready("paper", None, None),
                "const controls={paper:{disabled:false}};const loader={style:{display:'none',visibility:'visible'}};global.document={readyState:'complete',querySelector:()=>loader,getElementById:id=>controls[id]||null};global.getComputedStyle=e=>e.style;",
            ),
            "true"
        );
        assert_eq!(
            evaluate(
                &product_controls_ready("paper", Some("quantity"), None),
                "const controls={paper:{disabled:false},quantity:{disabled:false},effect:{disabled:true}};const loader={style:{display:'none',visibility:'visible'}};global.document={readyState:'complete',querySelector:()=>loader,getElementById:id=>controls[id]||null};global.getComputedStyle=e=>e.style;",
            ),
            "true"
        );
    }

    #[test]
    fn spinner_probe_queries_the_wait_marker() {
        assert_eq!(
            execute_and_observe(
                LOADING_SPINNER_VISIBLE,
                "let queried='';const marker={style:{display:'none',visibility:'visible'}};global.document={getElementById:id=>{queried=id;return marker}};global.getComputedStyle=e=>e.style;",
                "queried",
            ),
            "sysdiv_wait"
        );
    }

    #[test]
    fn select_helpers_choose_stock_bracket_and_foil_or_report_no_fit() {
        let setup = "let changes=[];const option=(text,value)=>({text,value});const controls={paper:{options:[option('Other','O'),option(' S33 ','S33')],selectedIndex:0,dispatchEvent:()=>changes.push('paper')},quantity:{options:[option('25','25'),option('100','100'),option('50','50')],selectedIndex:0,dispatchEvent:()=>changes.push('quantity')},effect:{value:'',dispatchEvent:()=>changes.push('effect')}};global.document={getElementById:id=>controls[id]};global.Event=function(type){this.type=type};";
        assert_eq!(
            execute_and_observe(
                &select_by_text("paper", "S33"),
                setup,
                "controls.paper.options[controls.paper.selectedIndex].value+':'+changes"
            ),
            "S33:paper"
        );
        assert_eq!(
            execute_and_observe(
                &select_smallest_bracket("quantity", 54),
                setup,
                "controls.quantity.options[controls.quantity.selectedIndex].value+':'+changes"
            ),
            "100:quantity"
        );
        assert_eq!(
            evaluate(&select_smallest_bracket("quantity", 101), setup),
            "false"
        );
        assert_eq!(
            execute_and_observe(
                &select_by_value("effect", "EF_055"),
                setup,
                "controls.effect.value+':'+changes"
            ),
            "EF_055:effect"
        );
    }

    #[test]
    fn configuration_check_retains_nonfoil_selection_and_checks_requested_foil() {
        let setup = "const option=(text,value)=>({text,value});const controls={paper:{options:[option('S33','s33')],selectedIndex:0},quantity:{options:[option('25','25'),option('100','100')],value:'100'},effect:{value:'normal'}};global.document={getElementById:id=>controls[id]};";
        assert_eq!(
            evaluate(
                &product_configuration_matches("paper", "S33", "quantity", 54, "effect", None),
                setup
            ),
            "true"
        );
        assert_eq!(
            evaluate(
                &product_configuration_matches(
                    "paper",
                    "S33",
                    "quantity",
                    54,
                    "effect",
                    Some("EF_055")
                ),
                setup
            ),
            "false"
        );
        assert_eq!(
            evaluate(
                &product_configuration_matches("paper", "S33", "quantity", 54, "effect", Some("EF_055")),
                "const option=(text,value)=>({text,value});const controls={paper:{options:[option('S33','s33')],selectedIndex:0},quantity:{options:[option('25','25'),option('100','100')],value:'100'},effect:{value:'EF_055'}};global.document={getElementById:id=>controls[id]};"
            ),
            "true"
        );
        assert_eq!(
            evaluate(
                &product_configuration_matches("paper", "S33", "quantity", 54, "effect", None),
                "const option=(text,value)=>({text,value});const controls={paper:{options:[option('Other','other')],selectedIndex:0},quantity:{options:[option('25','25'),option('100','100')],value:'100'},effect:{value:'normal'}};global.document={getElementById:id=>controls[id]};"
            ),
            "false"
        );
    }

    #[test]
    fn configuration_check_rejects_invalid_or_nonminimal_quantity_brackets() {
        for quantity in ["25", "not-a-number", "100"] {
            let setup = format!(
                "const option=(text,value)=>({{text,value}});const controls={{paper:{{options:[option('S33','s33')],selectedIndex:0}},quantity:{{options:[option('25','25'),option('75','75'),option('100','100')],value:'{quantity}'}}}};global.document={{getElementById:id=>controls[id]}};"
            );
            assert_eq!(
                evaluate(
                    &product_configuration_matches("paper", "S33", "quantity", 54, "effect", None),
                    &setup,
                ),
                "false",
                "quantity {quantity:?} must not match a requested quantity of 54"
            );
        }
    }

    #[test]
    fn back_chooser_acquisition_is_state_aware_and_mutations_revalidate() {
        let visible = "{style:{display:'block',visibility:'visible'}}";
        let base = |frame: &str, layout: &str| {
            format!(
                "global.location={{pathname:'/products/playingcard/design/dn_playingcards_back_dynamic.aspx'}};const frame={frame};const box={visible};global.document={{getElementById:id=>id==='sysifm_loginFrame'?frame:id==='sysdiv_loginFrame'?box:null}};global.getComputedStyle=e=>e.style;global.window=global;{layout}"
            )
        };

        // An absent prototype (the failing MPC initial state) and malformed
        // popup configuration are read-only non-openable states.
        assert_eq!(
            evaluate(BACK_CHOOSER_STATE, &base("null", "")),
            "uninitialized-layout"
        );
        assert_eq!(
            evaluate(
                BACK_CHOOSER_STATE,
                &base(
                    "null",
                    "global.PageLayout={prototype:{parameters:{ChangeDesignCount:{url:'/mode'}} ,renderDesignCount:()=>{}}};global.open=()=>{};",
                ),
            ),
            "uninitialized-layout"
        );

        let ready = "{style:{display:'block',visibility:'visible'},contentWindow:{location:{pathname:'/products/playingcard/design/dn_playingcards_mode_nb.aspx'},oRenderFeature:{},setMode:()=>{}}}";
        let ready_setup = base(
            ready,
            "let renders=0;global.PageLayout={prototype:{renderDesignCount:()=>{renders++}}};",
        );
        assert_eq!(evaluate(BACK_CHOOSER_STATE, &ready_setup), "ready");
        // The driver observes ready and does not invoke the opening mutation.
        assert_eq!(
            execute_and_observe(BACK_CHOOSER_STATE, &ready_setup, "renders"),
            "0"
        );

        let loading = "{style:{display:'block',visibility:'visible'},contentWindow:{location:{pathname:'/products/playingcard/design/dn_playingcards_mode_nb.aspx'}}}";
        let loading_setup = base(
            loading,
            "let renders=0;global.PageLayout={prototype:{renderDesignCount:()=>{renders++}}};",
        );
        assert_eq!(evaluate(BACK_CHOOSER_STATE, &loading_setup), "loading");
        assert_eq!(
            execute_and_observe(BACK_CHOOSER_STATE, &loading_setup, "renders"),
            "0",
            "an expected visible chooser still loading must not be reopened"
        );

        let layout = "let renders=0;global.open=()=>{};global.PageLayout={prototype:{parameters:{ChangeDesignCount:{url:'/mode',width:500,height:400}},renderDesignCount:()=>{renders++}}};";
        let openable = base("null", layout);
        assert_eq!(evaluate(BACK_CHOOSER_STATE, &openable), "openable");
        assert_eq!(
            execute_and_observe(&open_back_chooser(), &openable, "renders"),
            "1"
        );

        assert_eq!(
            evaluate(BACK_CHOOSER_STATE, "global.location={pathname:'/products/playingcard/design/dn_playingcards_front_dynamic.aspx'};global.document={getElementById:()=>null};global.window=global;"),
            "wrong-stage"
        );
        // A retained hidden frame is not an active chooser. A usable layout
        // can therefore open a fresh chooser.
        assert_eq!(
            evaluate(BACK_CHOOSER_STATE, &base("{style:{display:'none',visibility:'visible'},contentWindow:{location:{pathname:'/products/playingcard/design/dn_playingcards_mode_nb.aspx'}}}", layout)),
            "openable"
        );

        let mode_setup = base(
            "{style:{display:'block',visibility:'visible'},contentWindow:{location:{pathname:'/products/playingcard/design/dn_playingcards_mode_nb.aspx'},oRenderFeature:{},setMode:(...args)=>global.mode=args}}",
            "",
        );
        assert_eq!(
            execute_and_observe(&same_back_images(), &mode_setup, "mode.join(',')"),
            "ImageText,1"
        );
        assert_eq!(
            execute_and_observe(&different_back_images(), &mode_setup, "mode.join(',')"),
            "ImageText,0"
        );

        let thrown = node(
            &format!("{}\ntry{{eval(process.argv[1])}}catch(e){{process.stdout.write(e.message)}}", base("null", "global.open=()=>{};global.PageLayout={prototype:{parameters:{ChangeDesignCount:{url:'/mode',width:1,height:1}},renderDesignCount:()=>{throw new Error('render exploded')}}};")),
            &open_back_chooser(),
        );
        assert_eq!(
            thrown, "render exploded",
            "mutation exceptions must propagate"
        );
    }

    #[test]
    fn layout_and_slot_helpers_target_the_requested_slot_and_pid() {
        assert_eq!(
            evaluate(INITIAL_LAYOUT_HAS_SLOT_ONE, "global.PageLayout=undefined;"),
            "false",
            "a missing initial layout means there is no second slot"
        );
        assert_eq!(
            execute_and_observe(
                &slot_exists(7),
                "let requested='';global.PageLayout={prototype:{getElement3:(kind,slot)=>{requested=kind+':'+slot;return null}}};",
                "requested",
            ),
            "dnImg:7"
        );
        assert_eq!(
            execute_and_observe(
                &pid_in_slot(7),
                "let requested='';global.PageLayout={prototype:{getElement3:(kind,slot)=>{requested=kind+':'+slot;return {getAttribute:name=>name==='pid'?'wanted-pid':''}}}};",
                "requested",
            ),
            "dnImg:7"
        );
        assert_eq!(
            evaluate(
                &pid_in_slot(7),
                "global.PageLayout={prototype:{getElement3:()=>({getAttribute:name=>name==='pid'?'wanted-pid':''})}};",
            ),
            "wanted-pid"
        );
        assert_eq!(
            execute_and_observe(
                &insert_into_slot(7, "wanted-pid"),
                "let inserted='';global.PageLayout={prototype:{getElement3:(kind,slot)=>({kind,slot}),applyDragPhoto:(element,offset,pid)=>{inserted=element.kind+':'+element.slot+':'+offset+':'+pid}}};",
                "inserted",
            ),
            "dnImg:7:0:wanted-pid"
        );
    }

    #[test]
    fn editor_helpers_wait_for_input_then_update_quantity_and_mode() {
        let absent = "global.document={readyState:'complete',getElementById:()=>null};";
        assert_eq!(evaluate(EDITOR_QUANTITY_READY, absent), "false");
        let setup = "let events=[];let mode;const input={value:'',dispatchEvent:e=>events.push(e.type)};const frame={style:{display:'block',visibility:'visible'},contentWindow:{location:{pathname:'/products/playingcard/design/dn_playingcards_mode_nb.aspx'},document:{getElementById:id=>id==='txt_card_number'?input:null},oRenderFeature:{},setMode:(...args)=>mode=args}};const box={style:{display:'block',visibility:'visible'}};global.location={pathname:'/products/playingcard/design/dn_playingcards_back_dynamic.aspx'};global.document={readyState:'complete',getElementById:id=>id==='sysifm_loginFrame'?frame:id==='sysdiv_loginFrame'?box:null};global.getComputedStyle=e=>e.style;global.Event=function(type){this.type=type};";
        assert_eq!(evaluate(EDITOR_QUANTITY_READY, setup), "true");
        assert_eq!(
            execute_and_observe(
                &set_card_quantity(54),
                setup,
                "input.value+':'+events.join(',')"
            ),
            "54:input,change"
        );
        // Front mode is intentionally independent of the back chooser guard.
        let front_setup = setup.replace(
            "dn_playingcards_back_dynamic.aspx",
            "dn_playingcards_front_dynamic.aspx",
        );
        assert_eq!(
            execute_and_observe(&different_images(), &front_setup, "mode.join(',')"),
            "ImageText,0"
        );
        assert_eq!(
            execute_and_observe(&same_back_images(), setup, "mode.join(',')"),
            "ImageText,1"
        );
    }
}
