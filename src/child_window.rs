use slint;

slint::slint! {
import { TextEdit, Button } from "std-widgets.slint";
export component ChildWindow inherits Window {
    title: "Output";

    callback close-clicked();

    in property <string> child-text;
    in property <string> description;
    in property <int> init-height;
    
    preferred-height: init-height * 1px;

    VerticalLayout {
        Text {
            text: description;
        }
        if child-text != "" : TextEdit {
            wrap: no-wrap;
            text: child-text;
            read-only: true;
        }
        HorizontalLayout {
            Button {
                text: "Close";
                clicked => { close-clicked() }
            }
        }
    }
}
}