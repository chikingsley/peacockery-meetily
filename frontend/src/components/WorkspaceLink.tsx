import type { ComponentProps } from "react";

export function WorkspaceLink({
  href,
  children,
  onClick,
  ...props
}: ComponentProps<"a"> & { href: string }) {
  return (
    <a
      {...props}
      href={href}
      onClick={(event) => {
        onClick?.(event);

        if (
          event.defaultPrevented ||
          event.button !== 0 ||
          event.metaKey ||
          event.ctrlKey ||
          event.altKey ||
          event.shiftKey
        ) {
          return;
        }

        event.preventDefault();

        if (location.pathname + location.search !== href) {
          window.history.pushState(null, "", href);
        }
      }}
    >
      {children}
    </a>
  );
}
