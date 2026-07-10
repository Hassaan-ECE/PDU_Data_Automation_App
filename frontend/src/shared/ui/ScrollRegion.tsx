import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type ReactNode,
  type UIEventHandler,
} from "react";

import { cn } from "@/shared/lib/utils";

export type ScrollCue = {
  top: boolean;
  bottom: boolean;
};

type ScrollRegionProps = {
  children: ReactNode;
  className?: string;
  contentClassName?: string;
  "aria-label"?: string;
};

export function ScrollRegion({
  children,
  className,
  contentClassName,
  "aria-label": ariaLabel,
}: ScrollRegionProps) {
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const [scrollCue, setScrollCue] = useState<ScrollCue>({ top: false, bottom: false });

  const updateScrollCue = useCallback(() => {
    const element = scrollRef.current;
    if (!element) {
      setScrollCue({ top: false, bottom: false });
      return;
    }

    const overflow = element.scrollHeight > element.clientHeight + 1;
    const atTop = element.scrollTop <= 1;
    const atBottom = element.scrollTop + element.clientHeight >= element.scrollHeight - 1;
    const nextCue = {
      top: overflow && !atTop,
      bottom: overflow && !atBottom,
    };

    setScrollCue((current) =>
      current.top === nextCue.top && current.bottom === nextCue.bottom ? current : nextCue,
    );
  }, []);

  useEffect(() => {
    updateScrollCue();
    const element = scrollRef.current;
    if (!element || typeof ResizeObserver === "undefined") {
      return;
    }
    const observer = new ResizeObserver(() => updateScrollCue());
    observer.observe(element);
    if (element.firstElementChild) {
      observer.observe(element.firstElementChild);
    }
    return () => observer.disconnect();
  }, [updateScrollCue, children]);

  const onScroll: UIEventHandler<HTMLDivElement> = () => {
    updateScrollCue();
  };

  return (
    <div className={cn("relative min-h-0 flex-1 overflow-hidden", className)}>
      {scrollCue.top ? (
        <div className="pointer-events-none absolute inset-x-0 top-0 z-10 flex h-8 items-start justify-center bg-gradient-to-b from-[#20201f] to-transparent pt-1">
          <span className="h-0 w-0 border-x-[4px] border-b-[6px] border-x-transparent border-b-white/45" />
        </div>
      ) : null}
      <div
        ref={scrollRef}
        aria-label={ariaLabel}
        onScroll={onScroll}
        className="h-full overflow-y-auto [scrollbar-width:none] [&::-webkit-scrollbar]:hidden"
      >
        <div className={cn(contentClassName)}>{children}</div>
      </div>
      {scrollCue.bottom ? (
        <div className="pointer-events-none absolute inset-x-0 bottom-0 z-10 flex h-8 items-end justify-center bg-gradient-to-t from-[#20201f] to-transparent pb-1">
          <span className="h-0 w-0 border-x-[4px] border-t-[6px] border-x-transparent border-t-white/45" />
        </div>
      ) : null}
    </div>
  );
}
