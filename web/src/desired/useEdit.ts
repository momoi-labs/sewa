import { useMutation, useQueryClient } from '@tanstack/react-query';

import { addEdit, type Edit } from '../api';

/** Sends an edit to the draft change, then reads roles, hosts and changes again. */
export function useEdit() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (edit: Edit) => addEdit(edit),
    onSuccess: async () => {
      await Promise.all(['roles', 'hosts', 'changes'].map((key) => queryClient.invalidateQueries({ queryKey: [key] })));
    },
  });
}
