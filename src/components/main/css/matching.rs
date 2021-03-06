/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

// High-level interface to CSS selector matching.

use css::node_style::StyledNode;
use layout::extra::LayoutAuxMethods;
use layout::incremental;
use layout::util::LayoutDataAccess;
use layout::wrapper::LayoutNode;

use extra::arc::Arc;
use script::layout_interface::LayoutChan;
use servo_util::smallvec::SmallVec;
use style::{TNode, Stylist, cascade};
use style::{Before, After};

pub trait MatchMethods {
    fn match_node(&self, stylist: &Stylist);

    /// Performs aux initialization, selector matching, and cascading sequentially.
    fn match_and_cascade_subtree(&self,
                                 stylist: &Stylist,
                                 layout_chan: &LayoutChan,
                                 parent: Option<LayoutNode>);

    unsafe fn cascade_node(&self, parent: Option<LayoutNode>);
}

impl<'ln> MatchMethods for LayoutNode<'ln> {
    fn match_node(&self, stylist: &Stylist) {
        let style_attribute = self.with_element(|element| {
            match *element.style_attribute() {
                None => None,
                Some(ref style_attribute) => Some(style_attribute)
            }
        });

        let mut layout_data_ref = self.mutate_layout_data();
        match *layout_data_ref.get() {
            Some(ref mut layout_data) => {
                //FIXME To implement a clear() on SmallVec and use it(init_applicable_declarations).
                layout_data.data.init_applicable_declarations();

                stylist.push_applicable_declarations(self,
                                                     style_attribute,
                                                     None,
                                                     &mut layout_data.data.applicable_declarations);
                stylist.push_applicable_declarations(self,
                                                     None,
                                                     Some(Before),
                                                     &mut layout_data
                                                         .data
                                                         .before_applicable_declarations);
                stylist.push_applicable_declarations(self,
                                                     None,
                                                     Some(After),
                                                     &mut layout_data
                                                         .data
                                                         .after_applicable_declarations);
            }
            None => fail!("no layout data")
        }
    }

    fn match_and_cascade_subtree(&self,
                                 stylist: &Stylist,
                                 layout_chan: &LayoutChan,
                                 parent: Option<LayoutNode>) {
        self.initialize_layout_data((*layout_chan).clone());

        if self.is_element() {
            self.match_node(stylist);
        }

        unsafe {
            self.cascade_node(parent)
        }

        for kid in self.children() {
            kid.match_and_cascade_subtree(stylist, layout_chan, Some(*self))
        }
    }

    unsafe fn cascade_node(&self, parent: Option<LayoutNode>) {
        macro_rules! cascade_node(
            ($applicable_declarations: ident, $style: ident) => {{
                // Get our parent's style. This must be unsafe so that we don't touch the parent's
                // borrow flags.
                //
                // FIXME(pcwalton): Isolate this unsafety into the `wrapper` module to allow
                // enforced safe, race-free access to the parent style.
                let parent_style = match parent {
                    None => None,
                    Some(parent_node) => {
                        let parent_layout_data = parent_node.borrow_layout_data_unchecked();
                        match *parent_layout_data {
                            None => fail!("no parent data?!"),
                            Some(ref parent_layout_data) => {
                                match parent_layout_data.data.style {
                                    None => fail!("parent hasn't been styled yet?!"),
                                    Some(ref style) => Some(style.get()),
                                }
                            }
                        }
                    }
                };

                let computed_values = {
                    let layout_data_ref = self.borrow_layout_data();
                    let layout_data = layout_data_ref.get().as_ref().unwrap();
                    Arc::new(cascade(layout_data.data.$applicable_declarations.as_slice(),
                                     parent_style))
                };

                let mut layout_data_ref = self.mutate_layout_data();
                match *layout_data_ref.get() {
                    None => fail!("no layout data"),
                    Some(ref mut layout_data) => {
                        let style = &mut layout_data.data.$style;
                        match *style {
                            None => (),
                            Some(ref previous_style) => {
                                layout_data.data.restyle_damage = Some(incremental::compute_damage(
                                    previous_style.get(), computed_values.get()).to_int())
                            }
                        }
                        *style = Some(computed_values)
                    }
                }
            }}
        );

        {
            let before_len = {
                let layout_data_ref = self.borrow_layout_data();
                layout_data_ref.get().as_ref().unwrap().data.before_applicable_declarations.len()
            };
            if before_len > 0 {
                cascade_node!(before_applicable_declarations, before_style);
            }
        }
        cascade_node!(applicable_declarations, style);
        {
            let after_len = {
                let layout_data_ref = self.borrow_layout_data();
                layout_data_ref.get().as_ref().unwrap().data.after_applicable_declarations.len()
            };
            if after_len > 0 {
                cascade_node!(after_applicable_declarations, after_style);
            }
        }
    }
}

