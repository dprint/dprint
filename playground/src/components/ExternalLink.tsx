import React from "react";

export function ExternalLink(props: { text: string; url: string; id?: string; }): React.ReactElement {
    return <a id={props.id} href={props.url} rel="noopener noreferrer">{props.text}</a>;
}
