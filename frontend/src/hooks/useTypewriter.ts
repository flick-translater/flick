import { useEffect, useState, useRef, useCallback } from 'react';

interface UseTypewriterOptions {
  totalDuration?: number;
  minDuration?: number;
  maxDuration?: number;
  msPerUnit?: number;
  onComplete?: () => void;
  enabled?: boolean;
}

const CJK_CHAR_PATTERN = /[\u4e00-\u9fff\u3040-\u309f\u30a0-\u30ff\uac00-\ud7af]/;

function computeTextWeight(text: string) {
  let totalWeight = 0;

  for (const char of text) {
    totalWeight += CJK_CHAR_PATTERN.test(char) ? 1.5 : 1;
  }

  return totalWeight;
}

export function useTypewriter(
  text: string,
  options: UseTypewriterOptions = {}
) {
  const {
    totalDuration,
    minDuration = 450,
    maxDuration = 800,
    msPerUnit = 45,
    onComplete,
    enabled = true,
  } = options;
  const [displayedText, setDisplayedText] = useState('');
  const [isTyping, setIsTyping] = useState(false);
  const previousTextRef = useRef('');
  const timeoutRef = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);

  useEffect(() => {
    if (!enabled) {
      setDisplayedText(text);
      return;
    }

    if (text === previousTextRef.current) {
      return;
    }

    if (timeoutRef.current) {
      clearTimeout(timeoutRef.current);
    }

    previousTextRef.current = text;
    setDisplayedText('');
    setIsTyping(true);

    let currentIndex = 0;
    const textLength = text.length;
    const totalWeight = computeTextWeight(text);
    const resolvedDuration = totalDuration
      ?? Math.min(maxDuration, Math.max(minDuration, totalWeight * msPerUnit));
    const speed = totalWeight > 0 ? resolvedDuration / totalWeight : resolvedDuration;

    const typeNextChar = () => {
      if (currentIndex < textLength) {
        const char = text[currentIndex];
        setDisplayedText((prev) => prev + char);
        currentIndex++;

        const isCJK = CJK_CHAR_PATTERN.test(char);
        const delay = isCJK ? speed * 1.5 : speed;

        timeoutRef.current = setTimeout(typeNextChar, delay);
      } else {
        setIsTyping(false);
        onComplete?.();
      }
    };

    typeNextChar();

    return () => {
      if (timeoutRef.current) {
        clearTimeout(timeoutRef.current);
      }
    };
  }, [text, totalDuration, minDuration, maxDuration, msPerUnit, enabled, onComplete]);

  const skip = useCallback(() => {
    if (timeoutRef.current) {
      clearTimeout(timeoutRef.current);
    }
    setDisplayedText(text);
    setIsTyping(false);
    onComplete?.();
  }, [text, onComplete]);

  return { displayedText, isTyping, skip };
}
