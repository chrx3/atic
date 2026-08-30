/**
 * El float achicado no es una gota suelta: vive en la barra de la pill.
 *
 * Un blob propio en el grupo líquido se fundía con el launcher y los cupos, y
 * al mover la pill quedaba atrás. El estado vive acá para que la pill pinte la
 * pestaña y el float solo esconda el panel (las PTYs siguen).
 */

import { createDockExpand } from "./dockExpand";

class AgentsDock {
  minimized = $state(false);
  #expand = createDockExpand();

  bind(expand: () => void): () => void {
    return this.#expand.bind(expand);
  }

  setMinimized(on: boolean) {
    this.minimized = on;
  }

  expand() {
    this.#expand.call();
  }
}

export const agentsDock = new AgentsDock();
