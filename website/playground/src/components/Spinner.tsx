import { BeatLoader } from "react-spinners";

export function Spinner(props: { backgroundColor?: string }) {
  const { backgroundColor } = props;
  return (
    <div className="verticallyCenter horizontallyCenter fillHeight" style={{ backgroundColor }}>
      <BeatLoader color="#fff" loading size={25} />
    </div>
  );
}
