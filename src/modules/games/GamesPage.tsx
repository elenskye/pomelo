import { CollectionPage } from "@/shared/collection/CollectionPage";
import { GAMES, gamesApi } from "./config";

export default function GamesPage() {
  return <CollectionPage config={GAMES} api={gamesApi} />;
}
