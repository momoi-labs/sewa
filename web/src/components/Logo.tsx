import lockupDark from '../brand/lockup-dark.svg';
import lockupLight from '../brand/lockup.svg';
import markDark from '../brand/mark-dark.svg';
import markLight from '../brand/mark.svg';

export function BrandSymbol({ label = '' }: { label?: string }) {
  return (
    <span className="brand-symbol" aria-hidden={label ? undefined : 'true'}>
      <img className="brand-on-light" src={markLight} alt={label} />
      <img className="brand-on-dark" src={markDark} alt={label} />
    </span>
  );
}

export function Lockup() {
  return (
    <span className="brand-lockup">
      <img className="brand-on-light" src={lockupLight} alt="sewa" />
      <img className="brand-on-dark" src={lockupDark} alt="sewa" />
    </span>
  );
}
