import {
  Fragment,
  Transition,
  createVNode,
  defineComponent,
  getCurrentInstance,
  getTransitionRawChildren,
  resolveTransitionHooks,
  setTransitionHooks,
  useTransitionState,
  type BaseTransitionProps,
  type ComponentInternalInstance,
  type DefineComponent,
  type TransitionProps,
  type VNode,
} from 'vue'

/** `<Transition>`'s props but `mode`, which a list has no use for, as `<TransitionGroup>` drops it. */
const { mode: _mode, ...transitionProps } = (Transition as unknown as { props: Record<string, unknown> }).props

/**
 * A keyed list whose items enter and leave as `<TransitionGroup>`'s do, and
 * never animate a move.
 *
 * `<TransitionGroup>` reads every item's box as it renders, and after each
 * update clones an item into the page to read its styles, so as to animate
 * the items that moved. Each render of the list so forces the page's style
 * and layout, though a list with no move style animates none. A list that
 * animates no move takes this in its place: the same props as `<Transition>`
 * but `mode`, a name for its classes or `css: false` with hooks, and `tag` for
 * an element to hold the items, which takes the other attributes. Each item
 * is keyed; one without a key has no transition.
 */
export default defineComponent({
  name: 'ListTransition',
  props: {
    ...transitionProps,
    tag: String,
  },
  setup(props, { slots }) {
    const instance = getCurrentInstance() as ComponentInternalInstance
    const state = useTransitionState()
    /** The items of the last render, which an item that leaves now is one of. */
    let children: VNode[] = []
    return () => {
      const { tag, ...transition } = props
      // `<Transition>` turns its props, a name's classes or the hooks given,
      // into the base transition's, which each item takes.
      const resolved = (Transition as unknown as (props: TransitionProps, context: { slots: object }) => VNode)(
        transition as TransitionProps,
        { slots: {} },
      ).props as BaseTransitionProps
      // The last render's items take the props of this one, as in
      // `<TransitionGroup>`: one that leaves now leaves as they say.
      for (const child of children) {
        if (child.el instanceof Element) {
          setTransitionHooks(child, resolveTransitionHooks(child, resolved, state, instance))
        }
      }
      children = slots.default ? getTransitionRawChildren(slots.default()) : []
      for (const child of children) {
        if (child.key != null) {
          setTransitionHooks(child, resolveTransitionHooks(child, resolved, state, instance))
        }
      }
      return createVNode(tag ?? Fragment, null, children)
    }
  },
}) as unknown as DefineComponent<TransitionProps & { tag?: string }>
