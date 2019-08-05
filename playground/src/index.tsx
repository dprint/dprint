import React from "react";
import ReactDOM from "react-dom";
import "./index.css";
import { Playground } from "./Playground";
import * as serviceWorker from "./serviceWorker";

ReactDOM.render(<Playground />, document.getElementById("root"));

serviceWorker.unregister();
