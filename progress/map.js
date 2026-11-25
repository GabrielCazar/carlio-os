  (() => {
    const statuses = ["done", "doing", "pending", "blocked"];
    const labels = { done: "Validado", doing: "Em andamento", pending: "Pendente", blocked: "Dependência", archived: "Arquivado" };
    const phases = [...document.querySelectorAll(".phase")];
    const totals = Object.fromEntries(statuses.map(status => [status, 0]));
    const tree = document.getElementById("completion-tree");

    for (const phase of phases) {
      const heading = phase.querySelector(".phase-head h2").textContent.replace(/^\s*\d+\s*·\s*/, "").trim();
      const tasks = [...phase.querySelectorAll(".task")];
      const counts = Object.fromEntries(statuses.map(status => [status, 0]));
      const leaves = document.createElement("ul");
      leaves.className = "completion-branch";

      for (const task of tasks) {
        const status = statuses.find(candidate => task.classList.contains(candidate)) || (task.classList.contains("archived") ? "archived" : "pending");
        const name = task.querySelector("h3")?.textContent.trim() || "Tarefa sem título";
        if (status !== "archived") {
          counts[status]++;
          totals[status]++;
        }
        const leaf = document.createElement("li");
        leaf.className = `tree-task ${status}`;
        const marker = document.createElement("i");
        marker.className = "tree-state";
        marker.setAttribute("aria-hidden", "true");
        const taskName = document.createElement("span");
        taskName.className = "tree-task-name";
        taskName.textContent = name;
        const state = document.createElement("span");
        state.className = "tree-status";
        state.textContent = labels[status];
        leaf.append(marker, taskName, state);
        leaves.append(leaf);
      }

      const phaseTotal = statuses.reduce((sum, status) => sum + counts[status], 0);
      const branch = document.createElement("li");
      branch.className = "tree-phase";
      const details = document.createElement("details");
      const summary = document.createElement("summary");
      const phaseName = document.createElement("span");
      phaseName.className = "tree-phase-name";
      phaseName.textContent = heading;
      const phaseCounts = document.createElement("span");
      phaseCounts.className = "tree-phase-count";
      phaseCounts.textContent = `${counts.done}/${phaseTotal} validadas`;
      const mini = document.createElement("span");
      mini.className = "tree-mini";
      mini.setAttribute("aria-label", `Fase ${heading}: ${counts.done} de ${phaseTotal} tarefas validadas`);
      for (const status of statuses) {
        const segment = document.createElement("span");
        segment.className = status;
        segment.style.width = phaseTotal ? `${counts[status] / phaseTotal * 100}%` : "0%";
        mini.append(segment);
      }
      summary.append(phaseName, phaseCounts, mini);
      details.append(summary, leaves);
      branch.append(details);
      tree.append(branch);
    }

    const total = statuses.reduce((sum, status) => sum + totals[status], 0);
    const percent = total ? Math.floor(totals.done / total * 100) : 0;
    document.getElementById("completion-percent").textContent = `${percent}%`;
    document.getElementById("completion-summary").textContent = `${totals.done} de ${total} tarefas ativas validadas; tarefas arquivadas ficam fora do cálculo.`;
    const bar = document.getElementById("completion-bar");
    bar.setAttribute("aria-valuenow", String(percent));
    bar.setAttribute("aria-valuetext", `${percent}% concluído: ${totals.done} validadas de ${total} tarefas ativas`);
    for (const status of statuses) {
      const segment = bar.querySelector(`[data-status="${status}"]`);
      segment.style.width = total ? `${totals[status] / total * 100}%` : "0%";
      segment.title = `${labels[status]}: ${totals[status]}`;
    }
    const legend = document.getElementById("completion-legend");
    for (const status of statuses) {
      const item = document.createElement("span");
      const dot = document.createElement("i");
      dot.className = `dot ${status}`;
      dot.setAttribute("aria-hidden", "true");
      const count = document.createElement("b");
      count.textContent = String(totals[status]);
      item.append(dot, ` ${labels[status]} `, count);
      legend.append(item);
    }
  })();
