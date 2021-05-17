import React from "react";
import { BeatLoader } from "react-spinners";

export function Spinner(props: { backgroundColor?: string }) {
    const { backgroundColor } = props;
    return (
        <div className={"verticallyCenter horizontallyCenter fillHeight"} style={{ backgroundColor }}>
            <BeatLoader color={"#fff"} loading={true} size={25} />
        </div>
    );
}
