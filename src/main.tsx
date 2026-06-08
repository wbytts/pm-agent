import ReactDOM from "react-dom/client";
import { AppRouter } from "./router";
import "./styles.css";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <AppRouter />,
);
