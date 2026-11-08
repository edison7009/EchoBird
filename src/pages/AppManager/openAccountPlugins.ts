import { useNavigationStore } from '../../stores/navigationStore';

export async function openAccountPlugins() {
  useNavigationStore.getState().openAccountPlugins();
}
