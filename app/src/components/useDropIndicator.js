import { useLayoutEffect, useRef, useState } from "react";

const STRETCH_MS = 180;
const SETTLE_MS = 400;

// Механика "капли": замер активной кнопки и фазы stretch → settle → idle
export default function useDropIndicator({
  containerRef,
  buttonRefs,
  activeId,
  inset = 0,
  maxStretch = 1.2,
}) {
  const [indicator, setIndicator] = useState(null);
  const [phase, setPhase] = useState("idle");
  const [direction, setDirection] = useState("right");
  const lastRectRef = useRef(null);
  const timersRef = useRef([]);

  const measureFor = (id) => {
    const container = containerRef.current;
    const button = buttonRefs.current[id];
    if (!container || !button) return null;
    const containerRect = container.getBoundingClientRect();
    const buttonRect = button.getBoundingClientRect();
    return {
      left: buttonRect.left - containerRect.left - inset,
      width: buttonRect.width,
      rowWidth: containerRect.width - inset * 2,
    };
  };

  useLayoutEffect(() => {
    const target = measureFor(activeId);
    if (!target) return;
    const prev = lastRectRef.current;
    lastRectRef.current = target;

    timersRef.current.forEach(clearTimeout);
    timersRef.current = [];

    if (!prev || prev.left === target.left) {
      setIndicator(target);
      return;
    }

    const movingRight = target.left > prev.left;
    setDirection(movingRight ? "right" : "left");

    // maxStretch <= 1 — без хвоста: один плавный перелёт к цели
    if (maxStretch <= 1) {
      setPhase("settle");
      setIndicator(target);
      timersRef.current.push(setTimeout(() => setPhase("idle"), SETTLE_MS));
      return () => {
        timersRef.current.forEach(clearTimeout);
      };
    }

    // Фаза 1: капля вытягивается к цели (не длиннее maxStretch ширины кнопки),
    // передний край прижат к дальнему краю цели, хвост отрывается от старой кнопки
    const left = Math.min(prev.left, target.left);
    const right = Math.max(prev.left + prev.width, target.left + target.width);
    const stretchWidth = Math.min(right - left, target.width * maxStretch);
    setPhase("stretch");
    setIndicator({
      left: movingRight ? right - stretchWidth : left,
      width: stretchWidth,
      rowWidth: target.rowWidth,
    });

    // Фаза 2: схлопывается к новой кнопке с колыханием
    timersRef.current.push(
      setTimeout(() => {
        setPhase("settle");
        setIndicator(target);
      }, STRETCH_MS),
      setTimeout(() => setPhase("idle"), STRETCH_MS + SETTLE_MS),
    );

    return () => {
      timersRef.current.forEach(clearTimeout);
    };
  }, [activeId]);

  useLayoutEffect(() => {
    const onResize = () => {
      const target = measureFor(activeId);
      if (!target) return;
      lastRectRef.current = target;
      setPhase("idle");
      setIndicator(target);
    };
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, [activeId]);

  return { indicator, phase, direction };
}
