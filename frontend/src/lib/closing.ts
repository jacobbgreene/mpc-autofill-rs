export type CloseState = "idle" | "confirming" | "closing";
export type CloseContext = { dirty: boolean; running: boolean };

export function createCloseController(options: {
  context: () => CloseContext;
  confirm: (
    message: string,
    labels: { okLabel: string; cancelLabel: string },
  ) => Promise<boolean>;
  destroy: () => Promise<void>;
  changed: (state: CloseState, error: string) => void;
}) {
  let state: CloseState = "idle";
  return {
    async request() {
      if (state !== "idle") return;
      const { dirty, running } = options.context();
      state = dirty || running ? "confirming" : "closing";
      options.changed(state, "");
      try {
        if (dirty || running) {
          const accepted = await options.confirm(
            running
              ? `Closing stops autofill and closes its browser.${dirty ? " Unsaved order or draft changes will be discarded." : ""}`
              : "Discard your unsaved order or draft changes and close the app?",
            {
              okLabel: running
                ? "Stop autofill and close"
                : "Discard and close",
              cancelLabel: "Keep editing",
            },
          );
          if (!accepted) {
            state = "idle";
            options.changed(state, "");
            return;
          }
        }
        state = "closing";
        options.changed(state, "");
        await options.destroy();
      } catch (error) {
        state = "idle";
        options.changed(state, `Could not close the app: ${String(error)}`);
      }
    },
  };
}
