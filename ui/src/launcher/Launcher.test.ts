import { render, screen } from '@testing-library/svelte';
import Launcher from './Launcher.svelte';

test('the launcher renders a search input', () => {
  render(Launcher);
  expect(screen.getByRole('textbox')).toBeTruthy();
});
