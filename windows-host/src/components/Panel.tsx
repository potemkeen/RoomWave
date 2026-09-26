import React from "react";

interface PanelProps {
    title: string;
    onClose: () => void;
    children: React.ReactNode;
}

export function Panel({
                          title,
                          onClose,
                          children,
                      }: PanelProps) {
    const ref =
        React.useRef<HTMLDialogElement>(null);

    React.useEffect(() => {
        const dialog = ref.current!;

        dialog.showModal();

        return () => dialog.close();
    }, []);

    return (
        <dialog
            ref={ref}
            onCancel={onClose}
            aria-labelledby="panel-title"
            onClick={(event) => {
                if (event.target === event.currentTarget) {
                    onClose();
                }
            }}
        >
            <div className="panel-inner">
                <div className="panel-heading">
                    <h2 id="panel-title">{title}</h2>

                    <button
                        className="icon-button"
                        onClick={onClose}
                        aria-label="Закрыть"
                    >
                        ✕
                    </button>
                </div>

                {children}
            </div>
        </dialog>
    );
}
